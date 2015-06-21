Maybe you find something about the [[architecture]] helpful.
You can find some "how to's" in this wiki. They are linked in the navigation.

If you are using NetBeans IDE you can directly skip to this guides: [[Setup in NetBeans]]

## How to develop your own games using Wurfel Engine SDK (v1.3)
1. Download the Wurfel Engine SDK.
2. Include "WurfelEngine.jar" and the libGDX jars as libraries.
3. Your main class should look like this (if you start with a BasicMainMenu):

  ```java
  public static void main(String[] args) {
        WE.construct("Wurfelengine V" + WE.VERSION, args);
        
        BasicMenuItem[] menuItems = new BasicMenuItem[]{
            new BasicMenuItem(0, "Load Map", Controller.class, GameViewWithCamera.class, new Configuration()),
            new BasicMenuItem(1, "Map Editor", MapEditorController.class, MapEditorView.class, new Configuration()),
            new BasicMenuItem(2, "Options"),
            new BasicMenuItem(3, "Exit")
        };   
        
        WE.setMainMenu(new BasicMainMenu(menuItems));
        WE.launch();  
}
```
This is the code which is used in the `com.BombingGames.WurfelEngine.DesktopLauncher` class.

4. Programm your own [[Controller]] (game data & logic) and [[View]] (rendering, interface) as your starting point to your game.

  Your Controller must extend `com.BombingGames.WurfelEngine.Controller` and your view the `com.BombingGames.WurfelEngine.GameView`

  Extended explanation of this step: [[How to build a custom game]]

5. Optional: Change the engine configuration by extending `Configuration`.
6. Optional: Make your own [[main menu]] by replacing using the BasicMainMenu.

## How to build your own games (V1.1)

Everything works like above but you have to work from inside the engine source code. The usage like a library is not supported in this version. Take a look at the custom game example.
The block definitions must be done by hand inside the Block classes.

## How to use it on Android
[[Running the Wurfel Engine on Android]]

##How to use your own main menu.
Programm your own [[main menu]] (start by extending from BasicMainMenu.class) and pass it to the engine like this:
```java
public static void main(String[] args) {
    WEMain.construct("Wurfelengine V" + WEMain.VERSION, args);
    WEMain.setMainMenu(new CustomMainMenuScreen());
    WEMain.launch();        
}
```