An entity is an object which is not bound to the grid. Entities may move around (MovableEntitiy).

Entities must derive from _com.bombinggames.wurfelengine.core.AbstractEntity_.
![UML class diagram for AbstractEntity](https://github.com/BSVogler/WurfelEngineSDK/wiki/AbstractEntity.png)
They can be spawned to the map with the method `spawn(Point point)`

### Example
```java
Enemy enemy = (Enemy) new Enemy(). spawn( new Coordinate(5,5,5).getPoint() );
```

To remove the from the game you call `dispose()`. If you just want to remove them from the map call `removeFromMap()`.

## Sprites
By default entities use sprite category 'c'.
Sprite of entities with an id 0-9 are reserved by the engine.

Here is a list of the entities currently used by the engine.

0. invisible stuff
1. not in use
2. not in use
3. destruction decals
4. not in use
5. not in use
6. shadow
7. not in use
8. cursor
9. cursor normal

## Registering entities
You can register entities in order to spawn them in the editor.
`AbstractEntity.registerEntity(String name, Class<? extends AbstractEntity> entityClass)`;