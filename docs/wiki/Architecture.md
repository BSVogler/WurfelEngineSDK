An UML diagram showing the most important things:
![UML Dass diagram](https://github.com/BSVogler/WurfelEngineSDK/wiki/UML Class Diagramm.png)
The latest version on this UML diagram is included in the root folder.

The engine uses some sort of MVC. The data is managed in a "Controller" class while the input/output handling is managed in the view.
The view for example adds a camera, minimap, GUI etc. and reacts to key presses.

In the source code you will often find "handshakes" in the constructors. The classes reference each other before initializing. The init methods are in most times the real "constructors".