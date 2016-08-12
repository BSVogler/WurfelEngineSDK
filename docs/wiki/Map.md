The map is stored via [[chunk]]s.

You can set the amount of bytes which the engine can use for storing chunks with the [[CVar]] `mapMaxMemoryUse`. By having several thousand chunks in memory it is possible to calculate actions very far away. Objects outside the area near the camera are still active.

A RenderChunk is created for a chunk once it is near the camera in a 3x3 radius.

To show a minimal showing the loaded chunks set the [[CVar]] `showMiniMapChunk` to 1. RenderChunks are displayed in the color yellow.
