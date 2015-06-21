If you want to use custom [[blocks|Block]] you must _register_ them first. Wurfel Engine expects that every block has an unique id.

For that you need a class implementing _[BlockFactory](http://wurfelengine.net/javadoc/com/BombingGames/WurfelEngine/Core/Gameobjects/BlockFactory.html)_. 

## Example
```java
@Override
public Block produce(int id, int value, Coordinate coords) {
	Block block;
	switch (id){
		case 70:
			block = Block.createBasicInstance(id); 
			block.setTransparent(true);
			block.setNoSides();
		break;
		case 72:
			block = new AnimatedBlock(id, new int[]{1000,1000},true, true);//animation lighting
			block.setObstacle(true);
		break;
		default:
			Gdx.app.error("CustomBlockFactory", "Block "+id+"not defined.");
			block = Block.createBasicInstance(id);;
		}
		return block;
	}	
}
```

To use this class and register it as your block factory class set it 
```java
public class CustomConfiguration extends Configuration {
	private CustomBlockFactory factory = new CustomBlockFactory();

	@Override
	public BlockFactory getBlockFactoy() {
		return factory;
	}
}
```

Then you can use non-default blocks [[as usual |Block]] via 
```java
Block.getInstance(id)
```