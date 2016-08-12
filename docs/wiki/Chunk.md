A chunk is a three dimensional array containing map data.
The dimensions of a chunk are stored in the static fields `blocksX`, `blocksY`, `blocksZ`. The fields are set  by the map [[cvar]]s `chunkBlocksX`, `chunkBlocksY` & `chunkBlocksZ`;


By default a [[chunk]] is 40m\*10m\*10m\*3=12000 byte by a size of  400 m² area and 4000m³ volume.

# Wurfel Engine's approach
A classical (?) way to arrange the tiles is to create a square chunk and turn it about 45° degrees. This is called "diamond map".
![Square Map](http://upload.wikimedia.org/wikipedia/commons/e/e2/Findtilecoordinates.png)
But when you do this you have 50% out of the screen and x- and y-coordinates are also turned around 45°.

Using the "staggered map" format it is easier to use nine chunks for the map scrolling. It looks a bit like a brick wall.

With this, X and Y can be found like in every normal coordinate-system (Y mirrored).

The size of the chuks is very important because with higher chunks you get in danger that you see the highest blocks of the bottom chunks. When your chunk is too short in (Y-direction) it could happen that the top of a building suddenly appears during scrolling.

## Generators
Every chunk can be loaded from a file or generated. Wurfel Engine has some more or less simple chunk generators built in. You can set the generator with `GENERATOR`.

## Loading
A map consists of the chunk files and a meta file. The meta file includes the name of the map, the version and more information to come.
Every game can set it's own suffix' with:
`CHUNKFILESUFFIX`
'METAFILESUFFIX'