This guides were made using Netbeans 8 and uses Java 8.

## Working with the engine source
During game development you probably have to change the engine in some ways. Therefore I advice to use this setup. For simple setups the other setup may fit better.

### Setup Wurfel Engine
1. Create New Project -> Java -> Java Application
![NewProject](https://github.com/BSVogler/WurfelEngineSDK/wiki/NewProject.png)
2. Choose own name and location
3. Right click on the new project -> Properties.
4. Remove old source folder.
5. "Add Folder…" then choose `<WE SDK directory>/src`. You only need the source files so you can use hard links or copy the files wherever you like.
6. In the project overview: Right click on "Libraries" folder and add the libGDX files.

  ![Add Libraries Screenshot](https://github.com/BSVogler/WurfelEngineSDK/wiki/addLibraries.png)
  
  They should include:
    * gdx-controllers-desktop-natives.jar
    * gdx-controllers-desktop.jar
    * gdx-controllers.jar (in official libGDX release located under extensions/)
    * gdx-backend-lwjgl-natives.jar
    * gdx-backend-lwjgl.jar
    * gdx-natives.jar
    * gdx.jar

## Simple setup
**If you just want to make games and don't want to change the engine for your needs choose this guide.**
This will not include the source code of Wurfel Engine.

1. Create New Project -> Java -> Java Application
![NewProject](https://github.com/BSVogler/WurfelEngineSDK/wiki/NewProject.png)
2. Choose own name and location
3. In the project overview right click on Libraries.

  ![Add Libraries Screenshot](https://github.com/BSVogler/WurfelEngineSDK/wiki/addLibraries.png)
4. Add WurfelEngine.jar and the libGDX .jar files. They should be:
    * gdx-controllers-desktop-natives.jar
    * gdx-controllers-desktop.jar
    * gdx-controllers.jar
    * gdx-backend-lwjgl-natives.jar
    * gdx-backend-lwjgl.jar
    * gdx-natives.jar
    * gdx.jar