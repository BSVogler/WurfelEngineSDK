//! The sprite atlas as a texture: PNG decoding (pure, tested natively) and, in the browser build,
//! fetching the pages and uploading them as the layers of one texture array.
//!
//! The pages are decoded here instead of by the browser (`createImageBitmap`) so the pixels are
//! exact: no colour management, and no premultiplying that would blacken the edge pixels the
//! packer extruded around every sprite (linear filtering reads them).

/// A decoded image: RGBA, 8 bits per channel, rows top to bottom.
#[derive(Debug, PartialEq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Decode any PNG into [`Image`].
pub fn decode_png(bytes: &[u8]) -> Result<Image, String> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|e| format!("not a PNG: {e}"))?;
    let mut buffer = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buffer).map_err(|e| format!("corrupt PNG: {e}"))?;
    buffer.truncate(info.buffer_size());
    let pixels = info.width as usize * info.height as usize;
    let rgba = match info.color_type {
        png::ColorType::Rgba => buffer,
        png::ColorType::Rgb => buffer.chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        png::ColorType::GrayscaleAlpha => buffer.chunks_exact(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
        png::ColorType::Grayscale => buffer.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => return Err("indexed PNG was not expanded".into()),
    };
    if rgba.len() != pixels * 4 {
        return Err(format!("PNG data is {} bytes for {pixels} pixels", rgba.len()));
    }
    Ok(Image { width: info.width, height: info.height, rgba })
}

#[cfg(target_arch = "wasm32")]
pub use web::*;

#[cfg(target_arch = "wasm32")]
mod web {
    use wasm_bindgen::{JsCast, JsValue};
    use wasm_bindgen_futures::JsFuture;

    use super::decode_png;
    use crate::atlas::Atlas;
    use crate::sprites::Sprites;

    /// Where the atlas lives, relative to the page.
    const ATLAS_URL: &str = "assets/sprites/sprites.atlas";

    /// The layout of the atlas bind group: the texture array and its sampler (`shader.wgsl`, group 1).
    pub fn bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sprite atlas layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        })
    }

    fn array_texture(device: &wgpu::Device, width: u32, height: u32, layers: u32) -> wgpu::Texture {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some("sprite atlas"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: layers },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            // Not sRGB: the sprites are display colours and the surface is not sRGB either.
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        })
    }

    fn bind_group(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, texture: &wgpu::Texture) -> wgpu::BindGroup {
        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        // Linear like the Java atlas (`filter: Linear,Linear`); the packer's border keeps it clean.
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("sprite atlas sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sprite atlas"),
            layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&sampler) },
            ],
        })
    }

    fn upload(queue: &wgpu::Queue, texture: &wgpu::Texture, layer: u32, width: u32, height: u32, rgba: &[u8]) {
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x: 0, y: 0, z: layer },
                aspect: wgpu::TextureAspect::All,
            },
            rgba,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4 * width), rows_per_image: Some(height) },
            wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        );
    }

    /// A one pixel atlas, bound until the real one has arrived (the pipeline always needs something).
    pub fn placeholder(device: &wgpu::Device, queue: &wgpu::Queue, layout: &wgpu::BindGroupLayout) -> wgpu::BindGroup {
        let texture = array_texture(device, 1, 1, 1);
        upload(queue, &texture, 0, 1, 1, &[255, 0, 255, 255]);
        bind_group(device, layout, &texture)
    }

    /// The atlas keeps its file names when its content changes, so the browser must ask the server
    /// whether its copy is still good (`no-cache`) instead of reusing an old atlas: that one would
    /// miss the newer animation frames and the player would only ever show the walking ones.
    async fn fetch_bytes(url: &str) -> Result<Vec<u8>, String> {
        let window = web_sys::window().ok_or("no window")?;
        let init = web_sys::RequestInit::new();
        init.set_cache(web_sys::RequestCache::NoCache);
        let response: web_sys::Response = JsFuture::from(window.fetch_with_str_and_init(url, &init))
            .await
            .map_err(|e| format!("{url}: {}", js_message(&e)))?
            .dyn_into()
            .map_err(|_| format!("{url}: not a response"))?;
        if !response.ok() {
            return Err(format!("{url}: HTTP {}", response.status()));
        }
        let buffer = JsFuture::from(response.array_buffer().map_err(|e| js_message(&e))?)
            .await
            .map_err(|e| format!("{url}: {}", js_message(&e)))?;
        Ok(js_sys::Uint8Array::new(&buffer).to_vec())
    }

    fn js_message(e: &JsValue) -> String {
        e.as_string().or_else(|| js_sys::Reflect::get(e, &"message".into()).ok().and_then(|m| m.as_string())).unwrap_or_else(|| "failed".into())
    }

    /// Fetch the atlas and its pages, upload them, and return the sprites with the bind group.
    pub async fn load(device: &wgpu::Device, queue: &wgpu::Queue, layout: &wgpu::BindGroupLayout) -> Result<(Sprites, wgpu::BindGroup), String> {
        let text = fetch_bytes(ATLAS_URL).await?;
        let atlas = Atlas::parse(&String::from_utf8(text).map_err(|_| "the atlas is not text")?)?;
        let first = atlas.pages.first().ok_or("the atlas has no pages")?;
        let (width, height) = (first.width, first.height);
        if atlas.pages.iter().any(|p| (p.width, p.height) != (width, height)) {
            return Err("all atlas pages must have the same size".into());
        }
        let limits = device.limits();
        if width > limits.max_texture_dimension_2d || height > limits.max_texture_dimension_2d {
            return Err(format!("the atlas pages are {width}x{height}, this GPU allows {}", limits.max_texture_dimension_2d));
        }
        let base = ATLAS_URL.rsplit_once('/').map_or("", |(dir, _)| dir);
        let texture = array_texture(device, width, height, atlas.pages.len() as u32);
        for (layer, page) in atlas.pages.iter().enumerate() {
            let image = decode_png(&fetch_bytes(&format!("{base}/{}", page.file)).await?).map_err(|e| format!("{}: {e}", page.file))?;
            if (image.width, image.height) != (width, height) {
                return Err(format!("{} is {}x{}, the atlas says {width}x{height}", page.file, image.width, image.height));
            }
            upload(queue, &texture, layer as u32, width, height, &image.rgba);
        }
        let group = bind_group(device, layout, &texture);
        Ok((Sprites::new(atlas), group))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atlas::Atlas;

    fn shipped(file: &str) -> Vec<u8> {
        std::fs::read(format!("{}/assets/sprites/{file}", env!("CARGO_MANIFEST_DIR"))).unwrap()
    }

    /// Encode a tiny image, to test the decoder on every colour type.
    fn encode(color: png::ColorType, width: u32, height: u32, data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(color);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header().unwrap().write_image_data(data).unwrap();
        out
    }

    #[test]
    fn every_colour_type_decodes_to_rgba() {
        let rgba = decode_png(&encode(png::ColorType::Rgba, 2, 1, &[1, 2, 3, 4, 5, 6, 7, 8])).unwrap();
        assert_eq!(rgba, Image { width: 2, height: 1, rgba: vec![1, 2, 3, 4, 5, 6, 7, 8] });
        let rgb = decode_png(&encode(png::ColorType::Rgb, 2, 1, &[1, 2, 3, 4, 5, 6])).unwrap();
        assert_eq!(rgb.rgba, vec![1, 2, 3, 255, 4, 5, 6, 255]);
        let grey_alpha = decode_png(&encode(png::ColorType::GrayscaleAlpha, 1, 2, &[9, 100, 8, 200])).unwrap();
        assert_eq!(grey_alpha.rgba, vec![9, 9, 9, 100, 8, 8, 8, 200]);
        let grey = decode_png(&encode(png::ColorType::Grayscale, 2, 1, &[7, 250])).unwrap();
        assert_eq!(grey.rgba, vec![7, 7, 7, 255, 250, 250, 250, 255]);
    }

    #[test]
    fn garbage_is_an_error() {
        assert!(decode_png(b"not a png").unwrap_err().contains("not a PNG"));
        let mut cut = encode(png::ColorType::Rgba, 4, 4, &[0; 64]);
        cut.truncate(cut.len() - 20);
        assert!(decode_png(&cut).is_err());
    }

    #[test]
    fn the_shipped_pages_decode_to_the_size_the_atlas_says_and_hold_the_art() {
        let text = String::from_utf8(shipped("sprites.atlas")).unwrap();
        let atlas = Atlas::parse(&text).unwrap();
        for (index, page) in atlas.pages.iter().enumerate() {
            let image = decode_png(&shipped(&page.file)).unwrap();
            assert_eq!((image.width, image.height), (page.width, page.height), "{}", page.file);
            // Every sprite of the page has visible pixels inside its rectangle (the packing did
            // not leave an empty hole), and the region right of a sprite's border is transparent
            // or a neighbour, never garbage: check the sprite itself.
            for region in atlas.regions().iter().filter(|r| r.page == index) {
                // The player's charge and power overlays (`diff/s`, `diff/o`) are glows: translucent
                // everywhere, so any visible pixel counts.
                let glow = region.name.starts_with("diff/s/") || region.name.starts_with("diff/o/");
                let threshold = if glow { 0 } else { 127 };
                let mut opaque = 0;
                for y in region.y..region.y + region.h {
                    for x in region.x..region.x + region.w {
                        if image.rgba[((y * image.width + x) * 4 + 3) as usize] > threshold {
                            opaque += 1;
                        }
                    }
                }
                // `i10-0` and `diff/s/49` (one pixel) are fully transparent in the Java sheet too.
                assert!(opaque > 0 || ["i10-0", "diff/s/49"].contains(&region.name.as_str()), "{} is empty on its page", region.name);
            }
        }
    }

    #[test]
    fn the_grass_top_is_green_and_the_water_side_is_blue() {
        let text = String::from_utf8(shipped("sprites.atlas")).unwrap();
        let atlas = Atlas::parse(&text).unwrap();
        let average = |name: &str| {
            let region = atlas.region(name).unwrap();
            let image = decode_png(&shipped(&atlas.pages[region.page].file)).unwrap();
            let (mut sum, mut count) = ([0u64; 3], 0u64);
            for y in region.y..region.y + region.h {
                for x in region.x..region.x + region.w {
                    let i = ((y * image.width + x) * 4) as usize;
                    if image.rgba[i + 3] > 200 {
                        for c in 0..3 {
                            sum[c] += image.rgba[i + c] as u64;
                        }
                        count += 1;
                    }
                }
            }
            sum.map(|s| s as f32 / count as f32)
        };
        let grass = average("b1-0-1");
        assert!(grass[1] > grass[0] && grass[1] > grass[2], "grass top {grass:?}");
        let water = average("b9-0-0");
        assert!(water[2] > water[0] && water[2] > water[1], "water {water:?}");
    }
}
