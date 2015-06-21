Wurfel Engine separates the data managment from the rendering and access. It using some derivative of the MVC model.
In most clases you can find a render and a update method. This derives from this architecture. Update is always called first.

The View manages the interaction with the user or the AI.

The Controller manages and updates the game world.

![center of block](https://github.com/Cbeed/Wurfel-Engine/blob/1.2/docs/ControllerAndView.png)

The basic engine setup has two views and one controller. The engine view is in charge of game world independent things like the dev tools, game console etc. More important for your custom game is the class *GameView*. You should derive a class and use this as the view.

### Example
`CustomGameController` derives from `Controller`

`CustomGameView` derives from `GameView`

```java
CustomGameController ctrl = new CustomGameController();
WE.initGame(ctrl, new CustomGameView(ctrl), new CustomConfiguration());
```

Then you can place your game code in the update and render methods.