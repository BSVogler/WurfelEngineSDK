The block is similar to the blocks in Minecraft.

Every block has an `id` and a `value` and `health` property. Each field has 8bit, so values from 0 to 255 are possible.
In some parts of the engine the three values are combined in one _int_. It is useful when you have to pass more then one parameter i.e. `id` and `value`.

The `value` can store some information like the orientation (e.g. NPCs) or the color (e.g. wool).

The ids 0-9 are engine reserved blocks e.g. air (id 0).

[[RenderBlock]] objects are richer encapsulations of a cell with the three values. They are used for rendering and not for the game logic.

[[How to use custom blocks]]

## Spawning blocks
You can set them directly via:
```java
Controller.getMap().setBlock(…);
```
[Javadoc](http://wurfelengine.net/javadoc/com/BombingGames/WurfelEngine/Core/Map/Map.html#setData(int,%20int,%20int,%20com.BombingGames.WurfelEngine.Core.Gameobjects.Block))

or by using coordinates:
```java
new Coordinate(x, y, z, true).setBlock(Block); 
```
[javadoc](http://wurfelengine.net/javadoc/com/BombingGames/WurfelEngine/Core/Map/Coordinate.html#setBlock(com.BombingGames.WurfelEngine.Core.Gameobjects.Block))

## Dimensions
In most cases you are using game space coordinates. A list with important values can be found in the class RenderBlock.
`GAME_DIAGLENGTH` diagonal lenght of a block from front to back or left to right corner.
`GAME_EDGELENGTH` edge length of a block ^= height. So: 1m ~= `GAME_EDGELENGTH`

A block has the game space dimensions 1x1x1m = 1m^3.

The center of the block is in [0,0,0]. It is marked as the red dot in the picture.
![center of block](https://github.com/BSVogler/WurfelEngineSDK/wiki/center.png)

## Wrong isometric block projection
The internet is full of wrong isometric blocks which are not 1:1:1 and more like 1 : 0.8 : 0.8.
Wurfel Engine made the same mistake in the past. Not it is fixed.

## List of Engine reserved blocks
0 air

1 grass

2 dirt

3 stone

6 pavement

8 sand

9 water