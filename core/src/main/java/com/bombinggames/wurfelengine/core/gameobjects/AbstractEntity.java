/*
 * Copyright 2013 Benedikt Vogler.
 * All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions are met:
 *
 * * Redistributions of source code must retain the above copyright notice, 
 *   this list of conditions and the following disclaimer.
 * * Redistributions in binary form must reproduce the above copyright notice, 
 *   this list of conditions and the following disclaimer in the documentation 
 *   and/or other materials provided with the distribution.
 * * Neither the name of Bombing Games nor Benedikt Vogler nor the names of its contributors 
 *   may be used to endorse or promote products derived from this software without specific
 *   prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
 * AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE
 * LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
 * CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
 * SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
 * INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
 * CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
 * ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
 * POSSIBILITY OF SUCH DAMAGE.
 */
package com.bombinggames.wurfelengine.core.gameobjects;

import com.badlogic.gdx.Gdx;
import com.badlogic.gdx.ai.msg.Telegraph;
import com.bombinggames.wurfelengine.WE;
import com.bombinggames.wurfelengine.core.Controller;
import com.bombinggames.wurfelengine.core.map.Chunk;
import com.bombinggames.wurfelengine.core.map.Coordinate;
import com.bombinggames.wurfelengine.core.map.Point;
import com.bombinggames.wurfelengine.core.map.Position;
import com.bombinggames.wurfelengine.core.map.rendering.RenderCell;
import static com.bombinggames.wurfelengine.core.map.rendering.RenderCell.GAME_DIAGLENGTH2;
import static com.bombinggames.wurfelengine.core.map.rendering.RenderCell.GAME_EDGELENGTH;
import com.bombinggames.wurfelengine.core.map.rendering.RenderStorage;
import com.bombinggames.wurfelengine.core.sorting.TopoGraphNode;
import java.util.ArrayList;
import java.util.LinkedList;

/**
 * An entity is a game object which has the key feature that is has a position.
 *
 * @author Benedikt
 */
public abstract class AbstractEntity extends AbstractGameObject implements Telegraph {

	private static final long serialVersionUID = 2L;
	private static java.util.HashMap<String, Class<? extends AbstractEntity>> entityMap = new java.util.HashMap<>(10);//map string to class

	/**
	 *
	 */
	public final int colissionRadius = GAME_DIAGLENGTH2/2;

	/**
	 * Registers engine entities in a map.
	 */
	public static void registerEngineEntities() {
		entityMap.put("Explosion", Explosion.class);
		entityMap.put("Benchmarkball", BenchmarkBall.class);
	}
	
	/**
	 * Register a class of entities. The class must have a constructor without parameters.
	 * @param name the name of the entitie. e.g. "Ball"
	 * @param entityClass the class you want to register
	 */
	public static void registerEntity(String name, Class<? extends AbstractEntity> entityClass){
		entityMap.put(name, entityClass);	
	}
	
	/**
	 * Get a map of the registered entities
	 * @return 
	 */
	public static java.util.HashMap<String, Class<? extends AbstractEntity>> getRegisteredEntities() {
		return entityMap;
	}
	
	private float lightlevelR;
	private float lightlevelG;
	private float lightlevelB;
	private float health = 100f;
    private Point position;//the position in the map-grid
    private int dimensionZ = GAME_EDGELENGTH;  
    private boolean dispose;
	private boolean obstacle;
	private String name = "undefined";
	private boolean indestructible = false;
		/**
	 * time in ms to pass before new sound can be played
	 */
	private transient float soundTimeLimit;
	
	/**
	 * flags if should be saved
	 */
	private boolean savePersistent = true;
	private transient String[] damageSounds;
	private char spriteCategory = 'e';
	/**
	 * if true is not affected by physics time
	 */
	private boolean useRawDelta = false;
	private float mass = 0.4f;
	private final LinkedList<RenderCell> covered = new LinkedList<>();
	private final LinkedList<Component> components = new LinkedList<>();
	private byte spriteId;
	private byte value;
	
	/**
	 * can be used to save a heap call to obtain the coordiante.
	 * @see Coordinate#setFromPoint(com.bombinggames.wurfelengine.core.map.Point) 
	 * */
	private final Coordinate tmpCoordinate = new Coordinate(0, 0, 0);
	
	byte marked;
	/**
	 * Create an abstractEntity.
	 *
	 * @param spriteId objects with id = -1 will be deleted. 0 are invisible objects
	 */
	public AbstractEntity(byte spriteId) {
		super();
		this.spriteId = spriteId;
	}

	/**
	 * Create an abstractEntity.
	 *
	 * @param spriteId objects with id -1 are to be deleted. 0 are invisible objects
	 * @param value
	 */
	public AbstractEntity(byte spriteId, byte value) {
		super();
		this.spriteId = spriteId;
		this.value = value;
	}

	/**
	 * Updates the logic of the object.
	 *
	 * @param dt time since last update in game time
	 */
	public void update(float dt) {
		if (getHealth() <= 0 && !indestructible) {
			dispose();
		}

		if (soundTimeLimit > 0) {
			soundTimeLimit -= Gdx.graphics.getRawDeltaTime();
		}

		//update the components
		//question if traditional fore-loop is faster
		//http://stackoverflow.com/questions/16635398/java-8-iterable-foreach-vs-foreach-loop
		if (components.size() > 0) {
			@SuppressWarnings("unchecked")
			LinkedList<Component> cloneList = (LinkedList<Component>) components.clone();
			for (Component com : cloneList) {
				//if already in map don't update here
				if (!((com instanceof AbstractEntity)
					&& ((AbstractEntity) com).hasPosition()))
				com.update(dt);
			}
		}
	}
	
    //AbstractGameObject implementation
    @Override
    public final Point getPosition() {
        return position;
    }

	@Override
    public void setPosition(Position pos) {
        this.position = pos.toPoint();
    }

	/**
	 * keeps the reference
	 * @param pos 
	 */
	public void setPosition(Point pos) {
		this.position = pos;
	}

    /**
     * Is the entity laying/standing on the ground?
     * @return true when on the ground. False if in air or not in memory.
	 */
	public boolean isOnGround() {
		Point pos = getPosition();
		if (pos == null) {
			return false;
		} else {
			if (pos.getZ() <= 0) {
				return true; //if entity is under the map
			}
			if (pos.getZ() < Chunk.getGameHeight()) {
				//check if one pixel deeper is on ground.

				pos.setZ(pos.getZ() - 1);//move one up for check

				boolean colission = pos.isObstacle();
				pos.setZ(pos.getZ() + 1);//reverse

				return colission;
			} else {
				return false;//return false if over map
			}
		}
	}
    
	/**
	 * Add this entity to the map-&gt; let it spawn
	 *
	 * @param point the point in the game world where the object is. If it was
	 * previously set this is ignored.
	 * @return returns itself
	 */
	public AbstractEntity spawn(Point point) {
		if (position == null) {
			//spawn
			setPosition(point);
			dispose = false;
			Controller.getMap().addEntities(this);

			//request chunk if needed
			if (!position.isInMemoryAreaXY()) {
				this.requestChunk();
			}
		} else {
			WE.getConsole().add(getName() + " is already spawned.");
		}
		return this;
	}
	
	/**
	 * Is the object active on the map? If you spawn the object it has a
	 * position afterwards
	 *
	 * @return
	 */
	public boolean hasPosition() {
		return position != null;
	}

    @Override
    public char getSpriteCategory() {
        return spriteCategory;
    }
	
	/**
	 * Set the spriteCategory used for the lookup of the sprite.
	 *
	 * @param c
	 */
	public void setSpriteCategory(char c) {
		spriteCategory = c;
	}
    
 @Override
	public String getName() {
		if (name == null) {
			return "undefined";
		}
		return name;
	}
	
	/**
	 *
	 * @param name
	 */
	public void setName(String name) {
		this.name = name;
	}

	/**
	 * Set the height of the object.
	 *
	 * @param dimensionZ game space
	 */
	public void setDimensionZ(int dimensionZ) {
		this.dimensionZ = dimensionZ;
	}

	@Override
	public int getDimensionZ() {
		return dimensionZ;
	}

	/**
	 * Is the oject saved on the map?
	 *
	 * @return true if savedin map file.
	 */
	public boolean isSavedPersistent() {
		return savePersistent;
	}

	/**
	 * Mark objects to not be saved in disk. Gets passed to the children. Temp
	 * objects should not be saved.
	 *
	 * @param persisent new value of persisent
	 */
	public void setSavePersistent(boolean persisent) {
		this.savePersistent = persisent;
	}

	/**
	 * true if on chunk which is in memory
	 *
	 * @return
	 * @see com.bombinggames.wurfelengine.core.map.Coordinate#isInMemoryAreaXY()
	 */
	public boolean isInMemoryArea() {
		if (position == null) {
			return false;
		}
		return position.isInMemoryAreaXY();
	}

	/**
	 * Get the mass of the object.
	 * @return in kg
	 */
	public float getMass() {
		return mass;
	}

	/**
	 *
	 * @param mass in kg
	 */
	public void setMass(float mass) {
		this.mass = mass;
	}
	
	/**
	 * If the object can not be damaged. Object can still be disposed and
	 * removed from map.
	 *
	 * @return
	 */
	public boolean isIndestructible() {
		return indestructible;
	}

	/**
	 * If the object can not be damaged. Object can still be disposed and
	 * removed from map.
	 *
	 * @param indestructible
	 */
	public void setIndestructible(boolean indestructible) {
		this.indestructible = indestructible;
	}

	/**
	 * Called when gets damage. Health is between 0 and 100. Plays a sound. It
	 * is recommended to use an event to trigger the damaging so that each
	 * object manages it's own damage. If is set to indestructible via {@link #setIndestructible(boolean)
	 * } can not be damaged.
	 *
	 * @param value between 0 and 100
	 * @see #isIndestructible()
	 */
	public void takeDamage(byte value) {
		if (!indestructible) {
			if (health > 0) {
				if (damageSounds != null && soundTimeLimit <= 0) {
					//play random sound
					WE.SOUND.play(damageSounds[(int) (Math.random() * (damageSounds.length - 1))], getPosition());
					soundTimeLimit = 100;
				}
				setHealth(health - value);
			} else {
				setHealth((byte) 0);
			}
		}
	}
	
	/**
	 * 
	 * @param sound
	 */
	public void setDamageSounds(String[] sound) {
		damageSounds = sound;
	}
	
	/**
	 * Heals the entity. to-do: should be replaced with messages/events
	 *
	 * @param value
	 */
	public void heal(byte value) {
		if (getHealth() < 100) {
			setHealth((byte) (getHealth() + value));
		}
	}

	@Override
	public float getLightlevelR() {
		return lightlevelR;
	}

	@Override
	public float getLightlevelG() {
		return lightlevelG;
	}

	@Override
	public float getLightlevelB() {
		return lightlevelB;
	}

	@Override
	public void setLightlevel(float lightlevel) {
		this.lightlevelR = lightlevel;
		this.lightlevelG = lightlevel;
		this.lightlevelB = lightlevel;
	}

	/**
	 *
	 * @return from maximum 100
	 */
	public float getHealth() {
		return health;
	}
	
	/**
	 * clamps to [0..100]. You may prefer damage and {@link #heal(byte) }.
	 * Ignores invincibility.
	 *
	 * @param health
	 * @see #takeDamage(byte)
	 */
	public void setHealth(float health) {
		if (health > 100) {
			health = 100;
		}
		if (health < 0) {
			health = 0;
		}
		this.health = health;
	}

	/**
	 *
	 * @param useRawDelta
	 */
	public void setUseRawDelta(boolean useRawDelta) {
		this.useRawDelta = useRawDelta;
	}

	/**
	 *
	 * @return
	 */
	public boolean useRawDelta(){
		return useRawDelta;
	}

	/**
	 * loads the chunk at the position
	 */
	public void requestChunk() {
		if (hasPosition()) {
			Coordinate coord = getCoord();
			Chunk chunk = coord.getChunk();
			if (chunk == null) {
				int chunkX = coord.getChunkX();
				int chunkY = coord.getChunkY();
				if (!Controller.getMap().isLoading(chunkX, chunkY)) {
					WE.getConsole().add("Entity " + getName() + " requested chunk " + coord.getChunkX() + "," + coord.getChunkY());
					Controller.getMap().loadChunk(coord.getChunkX(), coord.getChunkY());
				}
			}
		}
	}

	/**
	 * *
	 * get every entity which is colliding
	 *
	 * @return
	 */
	public LinkedList<AbstractEntity> getCollidingEntities() {
		ArrayList<AbstractEntity> ents = Controller.getMap().getEntities();
		LinkedList<AbstractEntity> result = new LinkedList<>();//default size 5
		for (AbstractEntity entity : ents) {
			if (collidesWith(entity)) {
				result.add(entity);
			}
		}

		return result;
	}
	
	/**
	 * O(n) n:amount of entities. ignores if is obstacle.
	 *
	 * @param <T>
	 * @param filter only where the filter is true is returned
	 * @return
	 */
	public <T> LinkedList<T> getCollidingEntities(final Class<T> filter) {
		LinkedList<T> result = new LinkedList<>();//default size 5

		LinkedList<T> ents = Controller.getMap().getEntitys(filter);
		for (T entity : ents) {
			if (collidesWith(((AbstractEntity) entity))) {
				result.add(entity);
			}
		}

		return result;
	}

	/**
	 * spherical collision check
	 *
	 * @param ent
	 * @return
	 */
	public boolean collidesWith(AbstractEntity ent) {
		if (!ent.hasPosition()) {
			return false;
		}
		return getPosition().distanceToSquared(ent) < (colissionRadius + ent.colissionRadius) * (colissionRadius + ent.colissionRadius);
	}

	
	/**
	 * get the blocks which must be rendered before
	 *
	 * @param rs
	 * @return
	 */
	public LinkedList<RenderCell> getCoveredBlocks(RenderStorage rs) {
		covered.clear();
		if (position != null) {
			Coordinate coord = getCoord();
			RenderCell block;
			
			//add bottom layer
			block = rs.getCell(coord);//self
			if (block != null) {
				covered.add(block);
			}

			if (coord.getZ() < Chunk.getBlocksZ()) {
				block = rs.getCell(coord.goToNeighbour(7).add(0, 0, 1));//back left
				if (block != null) {
					covered.add(block);
				}

				block = rs.getCell(coord.goToNeighbour(2));//back right
				if (block != null) {
					covered.add(block);
				}
				coord.goToNeighbour(5).add(0, 0, -1);
			}
		
			if (coord.getZ() > 0){
				coord.add(0, 0, -1);//bottom
				block = rs.getCell(coord.goToNeighbour(5));//front left
				if (block != null) {
					covered.add(block);
				}

				block = rs.getCell(coord.goToNeighbour(3));//front
				if (block != null) {
					covered.add(block);
				}
				
				block = rs.getCell(coord.goToNeighbour(1));//front right
				if (block != null) {
					covered.add(block);
				}
				coord.goToNeighbour(7).add(0, 0, 1);
			}
		}
		return covered;
	}

	@Override
	public Point getPoint() {
		return position;
	}

	@Override
	public Coordinate getCoord() {
		return tmpCoordinate.setFromPoint(position);
	}

	/**
	 *
	 * @param component
	 */
	public void addComponent(Component component) {
		this.components.add(component);
		component.setParent(this);
	}

	/**
	 *
	 * @param <T>
	 * @param filterType
	 * @return
	 */
	public <T extends Component> Component getComponent(final Class<T> filterType) {
		for (Component comp : components) {
			if (filterType.isInstance(comp)) {
				return comp;
			}
		}
		return null;
	}

	/**
	 *
	 * @param component
	 */
	public void removeComponent(Component component) {
		this.components.remove(component);
	}
	
	/**
	 * false if in update list of map.
	 *
	 * @return true if disposing next tick
	 * @see #dispose()
	 */
	public boolean shouldBeDisposed() {
		return dispose;
	}
	
	/**
	 * Deletes the object from the map. The opposite to
	 * {@link #spawn(Point)}<br>
	 *
	 * @see #dispose()
	 * @see #spawn(Point)
	 */
	public void removeFromMap() {
		position = null;
	}

	/**
	 * Deletes the object from the map and every other container. The opposite
	 * to spawn() but also sets a flag to remove it completely.<br>
	 *
	 * @see #shouldBeDisposed()
	 * @see #removeFromMap()
	 */
	public void dispose() {
		dispose = true;
		removeFromMap();
	}

	@Override
	public byte getSpriteId() {
		return spriteId;
	}

	@Override
	public byte getSpriteValue() {
		return value;
	}
	
	/**
	 *
	 * @param id
	 */
	public void setSpriteId(byte id){
		if (id != this.spriteId) {
			this.spriteId = id;
			updateSpriteCache();
		}
	}
	
	/**
	 *
	 * @param value
	 */
	public void setSpriteValue(byte value){
		if (value != this.value) {
			this.value = value;
			updateSpriteCache();
		}
	}
	
	
		/**
	 * Check if it is marked in this frame. Used for depth sorting.
	 * @param id camera id
	 * @return 
	 * @see com.bombinggames.wurfelengine.core.sorting.TopologicalSort#visit(RenderCell) 
	 */
	public final boolean isMarkedDS(final int id) {
		return ((marked>>id)&1) == ((TopoGraphNode.currentMarkedFlag >> id) & 1);
	}

	/**
	 * Marks as visited in the depth sorting algorithm.
	 * @param id camera id
	 * @see com.bombinggames.wurfelengine.core.sorting.TopologicalSort#visit(RenderCell) 
	 */
	public void markAsVisitedDS(final int id) {
		marked ^= (-((TopoGraphNode.currentMarkedFlag >> id) & 1) ^ marked) & (1 << id);
	}
}
