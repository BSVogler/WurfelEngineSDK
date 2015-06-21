One special thing about Wurfel Engine is that the blocks have only three sides. Therefore you can calculate the light of every side like in an 3D game. This is done by the Light Engine (LE) which has been introduces in [V1.1]("Feature Update" V1.1).
In other games you can just brighten/darken the whole scene. 


The LE is in the package "com.BombingGames.Game.Lighting".
You can comment out the line 63 `useLightEngine(...)` in `com.BombingGames.Game.CustomGameController` to disable the use of the LE.

## Day/Night Cycle with global light sources
To simulate a day night cycle there are two global light sources: sun and moon.
The engine allows the easy use of more heavenly bodys but it wouldn't make any sense and it would be hard to understand that there are several suns or moons because you can never see the sun in isometric perspective.

The light engine uses the formula of phong shading on just three normals. With just three calculations  the whole calculation of the scene is done. 3D games do this some thousand times because every polygon is facing in a different direction.

=> Almost no performance difference with enabled lighting.

## local light sources
There were no researches made on this topic.
It would be possible but it would cost more performance then global light because the formula must be calculated at every surface. It will definitely look very good.

##Light color
The light color managment is at the moment relatively bad because every color has a brightness which get's ignored at the current state and therefore changes the result of the calculation of brightness.

##Diagrams
You can render some diagrams showing some data with `RenderData(true)`