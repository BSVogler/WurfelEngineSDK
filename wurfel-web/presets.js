/*
 * Graphics quality presets: what the Quality slider in the graphics options sets. The menu shows the
 * preset whose values all match the current settings and "Manual" as soon as any of them differs.
 * tools/graphics_bench.mjs reads this file too and times every preset (see benchmarks/).
 *
 * A preset lists every setting it controls. Not in a preset (the player's own taste, not a cost): wind
 * direction, cloud speed, the type of weather, linear blending, the frame rate limit.
 */
(function (root) {
  // Measured (benchmarks/graphics_bench.txt), GPU ms per frame at 3840x2160 with the sun up / down, the
  // sun-up number being the worst case (shadows only exist while the sun is up): Lowest 3 / 3, Low 7.5 / 3.5,
  // Medium 10 / 4, High 15 / 9, Ultra 26-34 / 10.5. A 1920x1080 screen has a quarter of the pixels.
  'use strict';
  root.wurfelPresets = [
    {
      name: 'Lowest',
      settings: {
        ambientOcclusion: false, sunShadows: false, shadowMethod: 'map', shadowSoftness: 0.4, shadowQuality: 'low',
        cloudShadows: false, bloom: 0, depthOfField: 0, fxaa: false, grass: false, grassDensity: 10,
        atmosphere: false, atmosphereDensity: 1, weatherDensity: 0.5, volumetrics: false, spriteShadows: false, waterReflection: false,
      },
    },
    {
      name: 'Low',
      settings: {
        ambientOcclusion: false, sunShadows: true, shadowMethod: 'map', shadowSoftness: 0.4, shadowQuality: 'low',
        cloudShadows: false, bloom: 0, depthOfField: 0, fxaa: true, grass: true, grassDensity: 4,
        atmosphere: false, atmosphereDensity: 1, weatherDensity: 0.5, volumetrics: false, spriteShadows: true, waterReflection: false,
      },
    },
    {
      name: 'Medium',
      settings: {
        ambientOcclusion: false, sunShadows: true, shadowMethod: 'map', shadowSoftness: 0.4, shadowQuality: 'medium',
        cloudShadows: true, bloom: 0.1, depthOfField: 0, fxaa: true, grass: true, grassDensity: 8,
        atmosphere: true, atmosphereDensity: 0.5, weatherDensity: 1, volumetrics: false, spriteShadows: true, waterReflection: true,
      },
    },
    {
      // The defaults a new player starts with.
      name: 'High',
      settings: {
        ambientOcclusion: false, sunShadows: true, shadowMethod: 'map', shadowSoftness: 0.4, shadowQuality: 'medium',
        cloudShadows: true, bloom: 0.1, depthOfField: 0.5, fxaa: true, grass: true, grassDensity: 10,
        atmosphere: true, atmosphereDensity: 1, weatherDensity: 1, volumetrics: true, spriteShadows: true, waterReflection: true,
      },
    },
    {
      // Voxel shadows at medium quality: the 'high' quality and softness above 40% cost twice as much
      // (benchmarks/graphics_bench.txt) for a slightly cleaner penumbra.
      name: 'Ultra',
      settings: {
        ambientOcclusion: true, sunShadows: true, shadowMethod: 'voxel', shadowSoftness: 0.4, shadowQuality: 'medium',
        cloudShadows: true, bloom: 0.15, depthOfField: 0.6, fxaa: true, grass: true, grassDensity: 20,
        atmosphere: true, atmosphereDensity: 1.5, weatherDensity: 1.5, volumetrics: true, spriteShadows: true, waterReflection: true,
      },
    },
  ];
})(typeof window !== 'undefined' ? window : globalThis);
