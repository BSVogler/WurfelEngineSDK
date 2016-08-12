The map editor can be opened by entering "editor" in the [[console]] ([[console commands]]) or calling the method `WE.startEditor()`. it can be leaved by pressing the ESC key.

![screenshot of editor tools](https://github.com/BSVogler/WurfelEngineSDK/wiki/editor tools.png)

The editor has some tools which work like in a drawing application.
# The Tools
* The _pen tool_ allows drawing of blocks. A "click drag" only uses the z level used for placing the first block when the mouse went down.
* The _bucket tool_ allows filling a rectangular field. Click, drag and release to fill an area.
* The _paint roller tool_ allows replacing the material of already set blocks.
* The _cursor tool_ allows selecting entities to move or delete them.
Selected entities can be moved when the cursor has the drag cursor.
Selected entities can be deleted by pressing the del key.
* The _entity tool_ allows placing entities. Entities must be registered to the editor. `AbstractEntity.registerEntity(String name, Class<? extends AbstractEntity> entityClass);`
* The _eraser tool_ is bound to the right mouse button. It can bind to the left mouse button by selecting it with the left mouse button

To change the value of a block or entity the text field below the tools can be used.

# Keyboard commands
* Q, R : move selected entities up or down
* Shift + WASD: scroll faster
* Command + Z: undo
* Command + Shift + Z: redo 
* DEL: delete entities
