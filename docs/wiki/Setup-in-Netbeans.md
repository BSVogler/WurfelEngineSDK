This guides were made using Netbeans 8.

## Game Developers only
**If you just want to make games and don't want to change the Engine for your needs choose this guide.**

1. Create New Project -> Java -> Java Application
2. Choose own name and location
3. In the project overview right click on Libraries.
4. Add Wurfel Engine .jar or add WE as a library if you configured it before using the libraries.

## Game Develoeprs + Engine Developers
**If you want to improve the engine or change it for your needs choose this guide.**

The following guide creates two separate projects. One is for the engine and one is for the game.
### Setup Wurfel Engine
1. Create New Project -> Java -> Java Application
2. Choose own name and location
3. Right click on the new project -> Properties.
4. Remove old source folder.
5. "Add Folder…" then choose `<WE source directory>/source`. If you don't do this and do not choose "/source/" every tool and library gets compiled with your build. So please do this. 
6. In the project overview: Right click on "Libraries" and add the libGDX files.

### Setup your game
1. Create new project.
2. Right click on Libraries -> "Add Project…", add the Wurfel Engine project you just created.
3. Also add "libGDX for We" again or the complete libGDX library.