The cvars are separated in three layers:

1. Engine Layer : Dependent on the loaded game
2. Map Layer: Dependent on the loaded map
3. Save Layer: Dependent on the loaded save

To access the cvars from code use
```java
WE.getCVars()
```
Cvars can be read ans set via the [[Console]]. You can use TAB to use the autocomplete feature.

# List of cvars
* timespeed <float>
* devmode <boolean>
* limitfps <int>
* lenormalmaprendering <boolean>
* music <float>
* sound <float>
* MaxSprites <int>
* CameraLeapRadius <int>
* undohistorySize <int>
* MaxDelta <float>
* DevDebugRendering <boolean>
* showMiniMapChunk <boolean>
etc.

For a full list look at the source code of the following files.
* _com.bombinggames.wurfelengine.core.cvar.CVarSystemRoot_
* _com.bombinggames.wurfelengine.core.cvar.CVarSystemMap_
* _com.bombinggames.wurfelengine.core.cvar.CVarSystemSave_