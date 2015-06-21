The block is similar to the blocks in Minecraft.

Block extends *AbstractGameObject*.
Please read the documentation for more information on the fields.


Every block has an **id** and a **value**.

The `value` can store some information like the orientation (e.g. NPCs) or the color (e.g. wool).

The ids <40 are reserved special blocks, like air (id 0).
[[How to use custom blocks]]

## Spawning blocks
You can create a block instance via 
```java
Block.getInstance(id)
```
[Javadoc](http://wurfelengine.net/javadoc/com/BombingGames/WurfelEngine/Core/Gameobjects/Block.html#getInstance(int))

You can set them directly via:
```java
Controller.getMap().setData(…);
```
[Javadoc](http://wurfelengine.net/javadoc/com/BombingGames/WurfelEngine/Core/Map/Map.html#setData(int,%20int,%20int,%20com.BombingGames.WurfelEngine.Core.Gameobjects.Block))

or by using coordinates:
```java
new Coordinate(x, y, z, true).setBlock(Block); 
```
[javadoc](http://wurfelengine.net/javadoc/com/BombingGames/WurfelEngine/Core/Map/Coordinate.html#setBlock(com.BombingGames.WurfelEngine.Core.Gameobjects.Block))

## Dimensions
In most cases you are using game space coordinates. A list with important values can be found in AbstractGameObject.
`GAME_DIAGLENGTH` diagonal lenght of a block from front to back or left to right corner.
`GAME_EDGELENGTH` edge length of a block ^= height. So: 1m ~= `GAME_EDGELENGTH`

A block has the game space dimensions 1:1:1m.

The center of the block is in [0,0,0]. It is marked as the red dot in the picture.
![center of block](https://github.com/Cbeed/Wurfel-Engine/blob/1.2/docs/center.png)

## Wrong isometric block projection
The internet is full of wrong isometric blocks which are not 1:1:1 and more like 1 : 0.8 : 0.8.
Wurfel Engine made the same mistake, also the project [Minecraft-Overviewer](https://github.com/overviewer/Minecraft-Overviewer/).

There is an issue stating this problem. See the issue: [Issue #12](https://github.com/Cbeed/Wurfel-Engine/issues/12)

## List of Engine reserved blocks
0 air

1 grass

2 dirt

3 stone

6 pavement

8 sand

9 water

34 flower

35 bush

36 tree

Caveland
41 cristall
42 sulfur
43 iron ore