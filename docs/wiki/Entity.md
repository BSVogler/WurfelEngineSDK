An entity is an object which is not bound to the grid.

Entities must derive from _AbstractEntity_.

They can be spawned to the map with the method `exist()`

### Example
```java
Enemy enemy = (Enemy) new Enemy(pos).exist();
```

## Reserved entities (v1.2)
Entities with an id <40 are reserved by the engine.

Here is a list of the entities currently used by the engine.

1. 
2. 
3. 
4. 
5. 
6. 
7. 
8. 
9. 
10. 
11. 
12. bullets
13. editor selector block
14. editor selector normal
15. bullet impact (white)
16. blood splash
17. blood floor decal
18. burned floor decal
19. bullet impact (yellow)
20. red dot
21. ball
22. 
23. 
24. 
25. 
26. 
27. 
28. 
29. 
30. Player
31. explosion
32. Character Shadow
...

## Registering entities
You can register entities in order to spawn them in the editor.
`AbstractEntity.registerEntity(String name, Class<? extends AbstractEntity> entityClass)`;