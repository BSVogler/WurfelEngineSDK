Last updated for 1.2.5:

The BasicMainMenu is just very basic and looks not good. Sooner or later you should use your own main menu.
You can use your own menu like this:

```java
    public static void main(String[] args) {
        WEMain.construct("Wurfelengine V" + WEMain.VERSION, args);
        WEMain.setMainMenu(new CustomMainMenuScreen());
        WEMain.launch();        
    }
```

To start the game from your own main menu use:
```java
WEMain.initGame(new CustomGameController(), new CustomGameView(),  new CustomConfiguration());
```

You also should add your custom files to the queue with
```java
WE.getAssetManager().load(String fileName, Class<T> type)
```
