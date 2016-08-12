If you want to understand the engine the article on the [[architecture]] may be helpful.
You can find some "how to's" in this wiki. They are linked in the navigation.

If you are using NetBeans IDE you can directly skip to this guides: [[Setup in NetBeans]]

## How to develop a game using Wurfel Engine SDK (v1.7)
1. Download the Wurfel Engine SDK.
2. Include "WurfelEngine.jar" and the libGDX jars as libraries.
3. Your main class should look like this (if you start with a BasicMainMenu):

  ```java
  public static void main(String[] args) {
        WorkingDirectory.setApplicationName("NameOfYourGame");
        
        BasicMenuItem[] menuItems = new BasicMenuItem[]{
             new BasicMenuItem(0, "Load Map", Controller.class, GameViewWithCamera.class),
             new BasicMenuItem(1, "Map Editor", Controller.class, EditorView.class),
             new BasicMenuItem(2, "Exit")
         };
        
        WE.setMainMenu(new BasicMainMenu(menuItems));
        WE.launch("NameOfYourGame - Made with WE V" + WE.VERSION, args);  
}
```
This code is similar to the one used in the class `com.BombingGames.WurfelEngine.DesktopLauncher`.

4. Programm your own [[Controller]] (game data & logic) and [[View]] (rendering, interface) as your starting point to your game.

  Your Controller must extend `com.BombingGames.WurfelEngine.Controller` and your view the `com.BombingGames.WurfelEngine.GameView`

  Extended explanation of this step: [[How to build a custom game]]

5. Optional: Change the engine configuration by modifying [[Cvars|CVar]]
6. Optional: Make your own [[main menu]] by replacing the BasicMainMenu.

##How to use your own main menu.
Programm your own [[main menu]] (start by extending from [AbstractMainMenu.html](http://wurfelengine.net/javadoc/com/bombinggames/wurfelengine/core/AbstractMainMenu.html)) and pass it to the engine like this:
```java
public static void main(String[] args) {
    WE.setMainMenu(new CustomMainMenuScreen());
    WE.launch("NameOfYourGame - Made with WE V" + WE.VERSION, args);  
}
```

## How to use it on Android (experimental)
[[Running the Wurfel Engine on Android]]