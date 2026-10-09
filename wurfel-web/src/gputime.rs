//! How long the GPU spends in each stage of a frame, measured by the GPU itself (WebGPU timestamp
//! queries on the render passes). Without the `timestamp-query` feature (WebGL2, or a browser that
//! does not offer it) nothing is measured and every call is free.
//!
//! Every pass asks [`GpuTimer::writes`] for its `timestamp_writes` with the stage it belongs to. After
//! the frame's commands are recorded, [`GpuTimer::resolve`] resolves the timestamps into a buffer that is
//! read back a few frames later (the GPU is never waited for). Per stage the time of its passes is attributed so that it
//! adds up to the frame (see `read`); [`GpuTimer::take`] gives the average per frame since the last call. The browser may round the
//! timestamps to 100 µs; the average over many frames is finer than that.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// Passes a frame may time (two timestamps each). The rest of a frame's passes are not measured.
const MAX_PASSES: u32 = 48;
/// Frames in flight: a frame's timestamps are read back this many frames later at the earliest.
const SLOTS: usize = 4;

struct Slot {
    buffer: wgpu::Buffer,
    /// The stages of the passes written to this slot, in query order (`None` while free).
    stages: RefCell<Option<Vec<&'static str>>>,
    ready: Rc<Cell<bool>>,
    mapping: Cell<bool>,
}

pub struct GpuTimer {
    queries: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    period_ns: f32,
    slots: Vec<Slot>,
    /// The stage of each pass recorded this frame.
    used: RefCell<Vec<&'static str>>,
    totals: RefCell<Totals>,
}

#[derive(Default)]
struct Totals {
    frames: u32,
    /// Sum per stage over the frames, in ms, in the order the stages first appeared.
    stages: Vec<(&'static str, f64)>,
    /// Sum of the time from a frame's first timestamp to its last one, in ms.
    span: f64,
}

/// The averages per frame: the GPU time per stage and in all, in ms.
#[derive(Clone, Debug, Default)]
pub struct GpuTimes {
    pub frames: u32,
    pub stages: Vec<(&'static str, f64)>,
    /// The GPU time of the frame: from the start of the first pass to the end of the last. The stages add up to it.
    pub busy: f64,
    /// Same as `busy` (kept for the status object).
    pub span: f64,
}

impl GpuTimer {
    /// The feature to ask the device for, when the adapter has it.
    pub fn feature(adapter: &wgpu::Adapter) -> wgpu::Features {
        adapter.features() & wgpu::Features::TIMESTAMP_QUERY
    }

    /// `None` when the device was made without the timestamp feature.
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Option<GpuTimer> {
        if !device.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
            return None;
        }
        let count = MAX_PASSES * 2;
        let size = u64::from(count) * 8;
        let queries = device.create_query_set(&wgpu::QuerySetDescriptor { label: Some("gpu timer"), ty: wgpu::QueryType::Timestamp, count });
        let resolve = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("gpu timer resolve"),
            size,
            usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let slots = (0..SLOTS)
            .map(|_| Slot {
                buffer: device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("gpu timer readback"),
                    size,
                    usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }),
                stages: RefCell::new(None),
                ready: Rc::new(Cell::new(false)),
                mapping: Cell::new(false),
            })
            .collect();
        Some(GpuTimer { queries, resolve, period_ns: queue.get_timestamp_period(), slots, used: RefCell::new(Vec::new()), totals: RefCell::new(Totals::default()) })
    }

    /// The `timestamp_writes` of the next pass, which belongs to `stage`.
    pub fn writes<'a>(timer: Option<&'a GpuTimer>, stage: &'static str) -> Option<wgpu::RenderPassTimestampWrites<'a>> {
        let timer = timer?;
        let mut used = timer.used.borrow_mut();
        let index = used.len() as u32;
        if index >= MAX_PASSES {
            return None;
        }
        used.push(stage);
        Some(wgpu::RenderPassTimestampWrites { query_set: &timer.queries, beginning_of_pass_write_index: Some(index * 2), end_of_pass_write_index: Some(index * 2 + 1) })
    }

    /// Call after the frame's passes are recorded and before the encoder is finished.
    pub fn resolve(&self, encoder: &mut wgpu::CommandEncoder) {
        let passes = self.used.borrow().len() as u32;
        let Some(slot) = self.slots.iter().find(|s| s.stages.borrow().is_none()).filter(|_| passes > 0) else {
            self.used.borrow_mut().clear();
            return;
        };
        let bytes = u64::from(passes) * 16;
        encoder.resolve_query_set(&self.queries, 0..passes * 2, &self.resolve, 0);
        encoder.copy_buffer_to_buffer(&self.resolve, 0, &slot.buffer, 0, bytes);
        *slot.stages.borrow_mut() = Some(std::mem::take(&mut *self.used.borrow_mut()));
    }

    /// Call after the commands are submitted: starts reading the slot written by `resolve` and picks up
    /// the ones that have arrived.
    pub fn collect(&self) {
        for slot in &self.slots {
            let Some(passes) = slot.stages.borrow().as_ref().map(Vec::len) else { continue };
            if !slot.mapping.get() {
                slot.mapping.set(true);
                let ready = slot.ready.clone();
                slot.buffer.slice(..passes as u64 * 16).map_async(wgpu::MapMode::Read, move |result| ready.set(result.is_ok()));
            } else if slot.ready.get() {
                self.read(slot, passes);
                slot.ready.set(false);
                slot.mapping.set(false);
                *slot.stages.borrow_mut() = None;
            }
        }
    }

    fn read(&self, slot: &Slot, passes: usize) {
        let stages = slot.stages.borrow();
        let Some(stages) = stages.as_ref() else { return };
        let times: Vec<u64> = {
            let Ok(view) = slot.buffer.slice(..passes as u64 * 16).get_mapped_range() else { return };
            view.chunks_exact(8).map(|b| u64::from_le_bytes(b.try_into().unwrap())).collect()
        };
        slot.buffer.unmap();
        let to_ms = |ticks: u64| ticks as f64 * f64::from(self.period_ns) / 1.0e6;
        let mut totals = self.totals.borrow_mut();
        // Passes of one frame overlap on the GPU, and a pass's start timestamp is taken early, so the
        // length of each pass added up is far more than the frame. A stage gets the time from the end of
        // the previous pass to the end of its own, which adds up to the span of the frame.
        let first = (0..stages.len()).map(|i| times[i * 2]).min().unwrap_or(0);
        let mut previous_end = first;
        for (i, stage) in stages.iter().enumerate() {
            let end = times[i * 2 + 1].max(previous_end);
            let ms = to_ms(end - previous_end);
            previous_end = end;
            match totals.stages.iter_mut().find(|(name, _)| name == stage) {
                Some((_, sum)) => *sum += ms,
                None => totals.stages.push((stage, ms)),
            }
        }
        totals.span += to_ms(previous_end - first);
        totals.frames += 1;
    }

    /// The average per frame since the last call (or `None` when no frame has arrived since).
    pub fn take(&self) -> Option<GpuTimes> {
        let mut totals = self.totals.borrow_mut();
        if totals.frames == 0 {
            return None;
        }
        let n = f64::from(totals.frames);
        let stages: Vec<(&'static str, f64)> = totals.stages.iter().map(|&(name, sum)| (name, sum / n)).collect();
        let times = GpuTimes { frames: totals.frames, busy: totals.span / n, stages, span: totals.span / n };
        *totals = Totals::default();
        Some(times)
    }
}
