Wurfel Engine supports a day night cycle with global light sources via pixel based shading. For the shading normal maps are needed.
The vertex based shading is not supported any more but still included in the source code.

The LE is in the package "com.bombinggames.wurfelengine.core.lightengine".

## Day/Night Cycle with Global Light Sources
To simulate a day night cycle there are two global light sources: sun and moon.

## Local Light Sources
When you want to have local lights e.g. a torch you can use the interface > `Coordinate.addLight(GameView view, Side side, int vertex, com.badlogic.gdx.graphics.Color color)`

[Javadoc](http://wurfelengine.net/javadoc/com/bombinggames/wurfelengine/core/map/Coordinate.html#addLight-com.bombinggames.wurfelengine.core.GameView-com.bombinggames.wurfelengine.core.gameobjects.Side-int-com.badlogic.gdx.graphics.Color-)

Local lights are very slow because they must relay on raytracing. Advantage is that global illumination via Radiosity is supported by the engine.

##Diagrams
You can render some diagrams showing some data with the console command "le".