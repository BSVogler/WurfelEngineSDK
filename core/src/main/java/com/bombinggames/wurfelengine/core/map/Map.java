/*
 * Copyright 2015 Benedikt Vogler.
 * All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions are met:
 *
 * * If this software is used for a game the official „Wurfel Engine“ logo or its name must be
 *   visible in an intro screen or main menu.
 * * Redistributions of source code must retain the above copyright notice,
 *   this list of conditions and the following disclaimer.
 * * Redistributions in binary form must reproduce the above copyright notice,
 *   this list of conditions and the following disclaimer in the documentation
 *   and/or other materials provided with the distribution.
 * * Neither the name of Benedikt Vogler nor the names of its contributors
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
package com.bombinggames.wurfelengine.core.map;

import com.badlogic.gdx.Gdx;
import com.badlogic.gdx.ai.msg.MessageManager;
import com.badlogic.gdx.ai.pfa.Connection;
import com.badlogic.gdx.ai.pfa.DefaultGraphPath;
import com.badlogic.gdx.ai.pfa.Heuristic;
import com.badlogic.gdx.ai.pfa.indexed.IndexedAStarPathFinder;
import com.badlogic.gdx.ai.pfa.indexed.IndexedGraph;
import com.badlogic.gdx.files.FileHandle;
import com.badlogic.gdx.utils.Array;
import com.bombinggames.wurfelengine.WE;
import com.bombinggames.wurfelengine.core.Controller;
import com.bombinggames.wurfelengine.core.Events;
import com.bombinggames.wurfelengine.core.GameView;
import com.bombinggames.wurfelengine.core.cvar.CVarSystemMap;
import com.bombinggames.wurfelengine.core.cvar.CVarSystemSave;
import com.bombinggames.wurfelengine.core.gameobjects.AbstractEntity;
import com.bombinggames.wurfelengine.core.map.Generators.AirGenerator;
import com.bombinggames.wurfelengine.core.map.rendering.RenderCell;
import java.io.File;
import java.io.IOException;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Collection;
import java.util.HashMap;
import java.util.LinkedList;
import java.util.logging.Level;
import java.util.logging.Logger;

/**
 * A map stores nine chunks as part of a bigger map. It also contains the
 * entities.
 *
 * @author Benedikt Vogler
 */
public class Map implements IndexedGraph<PfNode> {

	private static Generator defaultGenerator = new AirGenerator();

	/**
	 *
	 */
	public final static Integer MAPVERSION = 4;

	/**
	 *
	 * @param generator
	 */
	public static void setDefaultGenerator(Generator generator) {
		defaultGenerator = generator;
	}

	/**
	 * Get the default set generator.
	 *
	 * @return
	 * @see #setDefaultGenerator(Generator)
	 */
	public static Generator getDefaultGenerator() {
		return defaultGenerator;
	}

	/**
	 *
	 * @param path the directory of the map
	 * @return
	 */
	public static int newSaveSlot(File path) {
		int slot = getSavesCount(path);
		createSaveSlot(path, slot);
		return slot;
	}

	/**
	 *
	 * @param path the directory of the map
	 * @param slot
	 */
	public static void createSaveSlot(File path, int slot) {
		FileHandle pathHandle = Gdx.files.absolute(path + "/save" + slot + "/");
		if (!pathHandle.exists()) {
			pathHandle.mkdirs();
		}
		//copy from map folder root
		FileHandle root = Gdx.files.absolute(path.getAbsolutePath());
		FileHandle[] childen = root.list();
		for (FileHandle file : childen) {
			if (!file.isDirectory()) {
				file.copyTo(pathHandle);
			}
		}
	}

	/**
	 * Get the amount of save files for this map.
	 *
	 * @param path
	 * @return
	 */
	public static int getSavesCount(File path) {
		FileHandle children = Gdx.files.absolute(path.getAbsolutePath());
		int i = 0;
		while (children.child("save" + i).exists()) {
			i++;
		}
		return i;
	}

	/**
	 * every entity on the map is stored in this field
	 */
	private final ArrayList<AbstractEntity> entityList = new ArrayList<>(40);
	private boolean modified = true;
	/**
	 * observer pattern
	 */
	private Generator generator;
	private final File directory;
	private int activeSaveSlot;

	/**
	 * Stores the data of the map. Hash function is chunkX*chunkDim + chunkY. This means that there are only collision once you are outside the possible range specified in chunkDim. Was a 2d array before with same functionality but limited to dimensions of the array. HashMap now allows (slower) access to out-of-scope areas. TODO: Collisions can be avoided if areas, which are not active are pruned.
	 */
	private HashMap<Integer, Chunk> data;
	/**
	 * contains evey loaded chunk for fast iteration
	 */
	private LinkedList<Chunk> loadedChunks;
	
	private final ArrayList<ChunkLoader> loadingRunnables = new ArrayList<>(9);
	/**
	 * The amount of chunks in memory in one dimension.
	 */
	private final int chunkDim;
	/**
	 * Limits the amount of chunks which can be loaded into memory.
	 */
	private final int maxChunks;
	private final CVarSystemMap cVars;

	/**
	 * Loads a map using the default generator.
	 *
	 * @param name if available on disk it will be load
	 * @param saveslot
	 * @throws java.io.IOException thrown if there is no full read/write access
	 * to the map file
	 */
	public Map(final File name, int saveslot) throws IOException {
		this(name, saveslot, getDefaultGenerator());
	}

	/**
	 * Loads a map. Loads map and save cVars.
	 *
	 * @param name if available on disk it will load the meta file
	 * @param generator the generator used for generating new chunks
	 * @param saveSlot
	 * @throws java.io.IOException thrown if there is no full read/write access
	 * to the map file
	 */
	public Map(final File name, int saveSlot, Generator generator) throws IOException {
		this.directory = name;
		this.generator = generator;
		
		//init data array
		chunkDim = WE.getCVars().getValueI("mapIndexSpaceSize");
		data = new HashMap<>(chunkDim*chunkDim, 0.5f);
		
		maxChunks = WE.getCVars().getValueI("mapMaxMemoryUseBytes") / (Chunk.getBlocksX()*Chunk.getBlocksY()*Chunk.getBlocksZ()*3); //each block uses three bytes: id, sub id, health
		loadedChunks = new LinkedList<>();
		WE.getCVars().get("loadedMap").setValue(name.getName());
		
		//load map cVars
		cVars = new CVarSystemMap(new File(directory + "/meta.wecvar"));
		cVars.load();

		if (!hasSaveSlot(saveSlot)) {
			createSaveSlot(saveSlot);
		}
		useSaveSlot(saveSlot);

		Gdx.app.debug("Map", "Map named \"" + name + "\", saveslot " + saveSlot + " should be loaded");
	}

	/**
	 * 
	 * @return 
	 */
	public CVarSystemMap getCVars() {
		return cVars;
	}
	
	
	/**
	 *
	 * @return
	 */
	public CVarSystemSave getSaveCVars() {
		if (cVars == null) {
			return null;
		}
		return cVars.getSaveCVars();
	}

	/**
	 * Updates amostly the entities.
	 *
	 * @param dt time in ms
	 */
	public void update(float dt) {
		dt *= WE.getCVars().getValueF("timespeed");//apply game speed

		//add parralell loaded chunks serial to avoid conflicts
		for (int i = 0; i < loadingRunnables.size(); i++) {
			ChunkLoader runnable = loadingRunnables.get(i);
			if (runnable.getChunk() != null) {//loaded
				if (loadedChunks.size() < maxChunks ) {
					loadedChunks.add(runnable.getChunk());
					data.put(runnable.getCoordX()*chunkDim+runnable.getCoordY(), runnable.getChunk());
					addEntities(runnable.getChunk().retrieveEntities());
					setModified();
				}
				loadingRunnables.remove(i);
			}
		}
		
		for (Chunk chunk : loadedChunks) {
			if (chunk != null) {
				chunk.update(dt);
			}
		}

		//update every entity
		//old style for loop because allows modification during loop
		float rawDelta = Gdx.graphics.getRawDeltaTime() * 1000f;
		for (int i = 0; i < entityList.size(); i++) {
			AbstractEntity entity = entityList.get(i);
			if (!entity.isInMemoryArea()) {
				entity.requestChunk();
			}
			if (entity.useRawDelta()) {
				entity.update(rawDelta);
			} else {
				entity.update(dt);
			}
		}

		//remove not spawned objects from list
		entityList.removeIf((AbstractEntity entity) -> !entity.hasPosition());
	}

	/**
	 * Called after the view update to catch changes caused by the view
	 *
	 * @param dt
	 */
	public void postUpdate(float dt) {
		//check for modification flag
		for (Chunk chunk : loadedChunks) {
			if (chunk != null) {
				chunk.processModification();
			}
		}

		modificationCheck();
	}

	/**
	 * loads a chunk from disk if not already loaded.
	 *
	 * @param chunkX
	 * @param chunkY
	 */
	public void loadChunk(int chunkX, int chunkY) {
		if (loadedChunks.size() < maxChunks && Map.this.getChunk(chunkX, chunkY) == null) {
			if (!isLoading(chunkX, chunkY)) {
				ChunkLoader cl = new ChunkLoader(this, getPath(), chunkX, chunkY, getGenerator());
				loadingRunnables.add(cl);
				Thread thread = new Thread(cl, "loadChunk "+chunkX+","+chunkY);
				thread.start();
			}
		}
	}

	/**
	 * loads a chunk from disk if not already loaded.
	 *
	 * @param coord
	 */
	public void loadChunk(Coordinate coord) {
		loadChunk(coord.getChunkX(), coord.getChunkY());
	}
	/**
	 * Get the data of the map.
	 * From range in X [-chunkDim/2,chunkDim/2]
	 * @return
	 */
	public HashMap<Integer, Chunk> getData() {
		return data;
	}
	
	/**
	 *
	 * @return
	 */
	public LinkedList<Chunk> getLoadedChunks(){
		return loadedChunks;
	}

	/**
	 * Returns a block without checking the parameters first. Good for debugging
	 * and also faster. O(n)
	 *
	 * @param x coordinate
	 * @param y coordinate
	 * @param z coordinate
	 * @return the single block you wanted
	 */
	public byte getBlockId(final int x, final int y, final int z) {
		return (byte) (getBlock(x, y, z) & 255);
	}

	/**
	 * If the block can not be found returns null pointer.
	 *
	 * @param coord
	 * @return
	 */
	public byte getBlockId(final Coordinate coord) {
		return (byte) (getBlock(coord) & 255);
	}

	/**
	 * id, value and health
	 *
	 * @param coord
	 * @return
	 */
	public int getBlock(Coordinate coord) {
		if (coord.getZ() < 0) {
			return (byte) WE.getCVars().getValueI("groundBlockID");
		}
		Chunk chunk = getChunkContaining(coord);
		if (chunk == null) {
			return 0;
		} else {
			return chunk.getBlock(coord.getX(), coord.getY(), coord.getZ());//find chunk in x coord
		}
	}

	/**
	 *
	 * @param x
	 * @param y
	 * @param z
	 * @return
	 */
	public int getBlock(int x, int y, int z) {
		if (z < 0) {
			return (byte) WE.getCVars().getValueI("groundBlockID");
		}
		Chunk chunk = getChunkContaining(x, y);
		if (chunk == null) {
			return 0;
		} else {
			return chunk.getBlock(x, y, z);//find chunk in x coord
		}
	}

	/**
	 *
	 * @param coord
	 * @return
	 */
	public byte getHealth(Coordinate coord) {
		return (byte) ((getBlock(coord) >> 16) & 255);
	}

	/**
	 * Set a block at this coordinate. This creates a
	 * {@link AbstractBlockLogicExtension} instance if the block has logic.
	 *
	 * @param coord
	 * @param id
	 * @see #setBlock(Coordinate, int)
	 */
	public void setBlock(Coordinate coord, byte id) {
		Chunk chunk = getChunkContaining(coord);
		if (chunk != null) {
			chunk.setBlock(coord, id);
		}
	}
	
	/**
	 * Set id, value and health at a coordinate in the map. This creates a
	 * {@link AbstractBlockLogicExtension} instance if the block has logic.
	 *
	 * @param coord
	 * @param block id, value and health
	 */
	public void setBlock(Coordinate coord, int block) {
		Chunk chunk = getChunkContaining(coord);
		if (chunk != null && coord.getZ()>= 0 && coord.getZ() < Chunk.getBlocksZ()) {
			chunk.setBlock(coord, (byte) (block & 255), (byte) ((block >> 8) & 255), (byte) ((block >> 16) & 255));
		}
	}

	/**
	 * Set id and value at a coordinate in the map. This creates a
	 * {@link AbstractBlockLogicExtension} instance if the block has logic.
	 *
	 * @param coord
	 * @param id
	 * @param value
	 */
	public void setBlock(Coordinate coord, byte id, byte value) {
		Chunk chunk = getChunkContaining(coord);
		if (chunk != null) {
			chunk.setBlock(coord, id, value);
		}
	}

	/**
	 *
	 * @param coord
	 * @param value
	 */
	public void setValue(Coordinate coord, byte value) {
		getChunkContaining(coord).setValue(coord, value);//call to map
		//call to update RenderStorage
		GameView view = WE.getGameplay().getView();
		if (view != null) {//only update RS if can access it
			RenderCell renderCell = view.getRenderStorage().getCell(coord);
			if (renderCell != null) {
				renderCell.setValue(value);
			}
		}
	}

	/**
	 * Set health of a cell.
	 *
	 * @param coord
	 * @param health
	 */
	public void setHealth(Coordinate coord, byte health) {
		getChunkContaining(coord).setHealth(coord, health);
	}

	/**
	 * get the chunk where the coordinates are on. Usese hashmap so O(1).
	 *
	 * @param coord not altered
	 * @return can return null if not loaded
	 */
	public Chunk getChunkContaining(final Coordinate coord) {
		return data.get(Math.floorDiv(coord.getX(), Chunk.getBlocksX())*chunkDim + Math.floorDiv(coord.getY(), Chunk.getBlocksY()));
	
	}

	/**
	 * get the chunk where the coordinates are on
	 *
	 * @param x grid coordinate
	 * @param y grid coordinate
	 * @return can return null if not loaded
	 */
	public Chunk getChunkContaining(int x, int y) {
		return data.get(Math.floorDiv(x, Chunk.getBlocksX())*chunkDim + Math.floorDiv(y, Chunk.getBlocksY()));
	}
	
	/**
	 * 
	 * @param point
	 * @return 
	 */
	public Chunk getChunkContaining(Point point) {
		//bloated in-place code to avoid heap call with toCoord()
		int xCoord = Math.floorDiv((int) point.getX(), RenderCell.GAME_DIAGLENGTH);
		int yCoord = Math.floorDiv((int) point.getY(), RenderCell.GAME_DIAGLENGTH) * 2 + 1; //maybe dangerous to optimize code here!
		//find the specific coordinate (detail)
		switch (Coordinate.getNeighbourSide(point.getX() % RenderCell.GAME_DIAGLENGTH,
			point.getY() % RenderCell.GAME_DIAGLENGTH
		)) {
			case 0:
				yCoord -= 2;
				break;
			case 1:
				xCoord += yCoord % 2 == 0 ? 0 : 1;
				yCoord--;
				break;
			case 2:
				xCoord++;
				break;
			case 3:
				xCoord += yCoord % 2 == 0 ? 0 : 1;
				yCoord++;
				break;
			case 4:
				yCoord += 2;
				break;
			case 5:
				xCoord -= yCoord % 2 == 0 ? 1 : 0;
				yCoord++;
				break;
			case 6:
				xCoord--;
				break;
			case 7:
				xCoord -= yCoord % 2 == 0 ? 1 : 0;
				yCoord--;
				break;
		}

		return getChunkContaining(xCoord, yCoord);
	}

	/**
	 * get the chunk with the given chunk coords.<br><br> Runtime: O(1)
	 *
	 * @param chunkX chunk coordinate
	 * @param chunkY chunk coordinate
	 * @return if not in memory returns null
	 */
	public Chunk getChunk(int chunkX, int chunkY) {
		return data.get(chunkX*chunkDim + chunkY);//this is the hash function
	}

	/**
	 * Get every entity on a chunk.
	 *
	 * @param xChunk
	 * @param yChunk
	 * @return
	 */
	public ArrayList<AbstractEntity> getEntitiesOnChunk(final int xChunk, final int yChunk) {
		ArrayList<AbstractEntity> list = new ArrayList<>(10);

		//loop over every loaded entity
		for (AbstractEntity ent : getEntities()) {
            if (
					ent.hasPosition()
				&&
					ent.getPosition().getX() > xChunk*Chunk.getGameWidth()//left chunk border
                &&
					ent.getPosition().getX() < (xChunk+1)*Chunk.getGameWidth() //left chunk border
				&&
					ent.getPosition().getY() > (yChunk)*Chunk.getGameDepth()//top chunk border
				&&
					ent.getPosition().getY() < (yChunk+1)*Chunk.getGameDepth()//top chunk border
            ){
				list.add(ent);//add it to list
			}
		}

		return list;
	}

	/**
	 * Get every entity on a chunk which should be saved
	 *
	 * @param xChunk
	 * @param yChunk
	 * @return
	 */
	public ArrayList<AbstractEntity> getEntitiesOnChunkSavedOnly(final int xChunk, final int yChunk) {
		ArrayList<AbstractEntity> list = new ArrayList<>(10);

		//loop over every loaded entity
		for (AbstractEntity ent : getEntities()) {
            if (
					ent.isSavedPersistent() && ent.hasPosition() //save only entities which are flagged
				&&
					ent.getPosition().getX() > xChunk*Chunk.getGameWidth()//left chunk border
                &&
					ent.getPosition().getX() < (xChunk+1)*Chunk.getGameWidth() //left chunk border
				&&
					ent.getPosition().getY() > (yChunk)*Chunk.getGameDepth()//top chunk border
				&&
					ent.getPosition().getY() < (yChunk+1)*Chunk.getGameDepth()//top chunk border
            ){
				list.add(ent);//add it to list
			}
		}

		return list;
	}

	/**
	 * saves every chunk on the map
	 *
	 * @param saveSlot
	 * @return
	 */
	public boolean save(int saveSlot) {
		for (Chunk chunk : loadedChunks) {
			try {
				chunk.save(
					this,
					getPath(),
					saveSlot
				);
			} catch (IOException ex) {
				Logger.getLogger(Map.class.getName()).log(Level.SEVERE, null, ex);
				return false;
			}
		}
		return true;
	}

	/**
	 * save every chunk using the current active save slot. Saves position of
	 * the sun and moon at origin.
	 *
	 * @return
	 */
	public boolean save() {
		getSaveCVars().get("LEsunAzimuth").setValue(Controller.getLightEngine().getSun(new Coordinate(0, 0, 0)).getAzimuth());
		getSaveCVars().get("LEmoonAzimuth").setValue(Controller.getLightEngine().getMoon(new Coordinate(0, 0, 0)).getAzimuth());
		return save(activeSaveSlot);
	}

	/**
	 *
	 * @param coord
	 * @return
	 */
	public AbstractBlockLogicExtension getLogic(Coordinate coord) {
		Chunk chunk = getChunkContaining(coord);
		if (chunk == null) {
			return null;
		} else {
			return chunk.getLogic(coord);
		}
	}

	/**
	 * Add a logicblock to the map.
	 *
	 * @param block
	 */
	public void addLogic(AbstractBlockLogicExtension block) {
		Chunk chunk = getChunkContaining(block.getPosition());
		chunk.addLogic(block);
	}

	/**
	 * uses a specific save slot for loading and saving the map. Loads the save
 cVars.
	 *
	 * @param slot slot number
	 */
	public void useSaveSlot(int slot) {
		this.activeSaveSlot = slot;
		cVars.get("currentSaveSlot").setValue(slot);
		//load save cVars
		cVars.setSaveCVars(
			new CVarSystemSave(
				new File(directory + "/save" + activeSaveSlot + "/meta.wecvar")
			)
		);
		cVars.load();
	}

	/**
	 * Uses a new save slot as the save slot
	 *
	 * @return the new save slot number
	 */
	public int newSaveSlot() {
		useSaveSlot(getSavesCount());
		createSaveSlot(activeSaveSlot);
		return activeSaveSlot;
	}

	/**
	 * Check if the save slot exists.
	 *
	 * @param saveSlot
	 * @return
	 */
	public boolean hasSaveSlot(int saveSlot) {
		return Gdx.files.absolute(directory + "/save" + saveSlot).exists();
	}

	/**
	 *
	 * @param slot
	 */
	public void createSaveSlot(int slot) {
		createSaveSlot(directory, slot);
	}

	/**
	 * checks a map for the amount of save files
	 *
	 * @return the amount of saves for this map
	 */
	public int getSavesCount() {
		return getSavesCount(directory);
	}

	/**
	 * should be executed after the update method
	 */
	public void modificationCheck() {
		if (modified) {
			MessageManager.getInstance().dispatchMessage(Events.mapChanged.getId());
			modified = false;
		}
	}

	/**
	 *
	 * @return
	 */
	public Generator getGenerator() {
		return generator;
	}

	/**
	 *
	 * @return
	 */
	public int getCurrentSaveSlot() {
		return activeSaveSlot;
	}

	/**
	 * Set the generator used for generating maps
	 *
	 * @param generator
	 */
	public void setGenerator(Generator generator) {
		this.generator = generator;
	}

	/**
	 * The name of the map on the file.
	 *
	 * @return
	 */
	public File getPath() {
		return directory;
	}

	/**
	 * set the modified flag to true. usually not manually called.
	 */
	public void setModified() {
		this.modified = true;
	}

	/**
	 * Returns a coordinate pointing to the absolute center of the map. Height
	 * is half the map's height.
	 *
	 * @return
	 */
	public Point getCenter() {
		return getCenter(Chunk.getBlocksZ() * RenderCell.GAME_EDGELENGTH / 2);
	}

	/**
	 * Returns a coordinate pointing to middle of a 3x3 chunk map.
	 *
	 * @param height You custom height.
	 * @return
	 */
	public Point getCenter(final float height) {
		return new Point(
			Chunk.getGameWidth() / 2,
			Chunk.getGameDepth() / 2,
			height
		);
	}

	/**
	 * Returns a copy of the entityList.
	 *
	 * @return every item on the map
	 */
	public ArrayList<AbstractEntity> getEntities() {
		return entityList;
	}

	/**
	 * Adds entities.
	 *
	 * @param ent entities should be already spawned
	 */
	public void addEntities(AbstractEntity... ent) {
		//remove duplicates
		for (AbstractEntity e : ent) {
			entityList.remove(e);
		}
		entityList.addAll(Arrays.asList(ent));
	}
	
	/**
	 * Adds entities.
	 *
	 * @param ent entities should be already spawned
	 */
	public void addEntities(Collection<AbstractEntity> ent) {
		if (ent != null) {
			//remove duplicates
			for (AbstractEntity e : ent) {
				entityList.remove(e);
			}
			entityList.addAll(ent);
		}
	}
	

	/**
	 * Disposes every entity on the map and clears the list.
	 */
	public void disposeEntities() {
		entityList.forEach((AbstractEntity e) -> e.dispose());
		entityList.clear();
	}

	/**
	 * Find every instance of a special class. E.g. find every
	 * <i>AbstractCharacter</i>. They must be spawned to appear in the results.
	 *
	 * @param <T> the class you want to filter.
	 * @param filter the class you want to filter.
	 * @return a list with the entitys
	 */
	@SuppressWarnings(value = {"unchecked"})
	public <T> LinkedList<T> getEntitys(final Class<T> filter) {
		LinkedList<T> result = new LinkedList<>();
		if (filter == null) {
			throw new IllegalArgumentException();
		}
		for (AbstractEntity entity : entityList) {
			if (entity.hasPosition() && filter.isInstance(entity)) {
				result.add((T) entity);
			}
		}
		return result;
	}

	/**
	 * Get every entity on a coord.
	 *
	 * @param coord
	 * @return a list with the entitys
	 */
	public LinkedList<AbstractEntity> getEntitysOnCoord(final Coordinate coord) {
		LinkedList<AbstractEntity> result = new LinkedList<>();

		for (AbstractEntity ent : entityList) {
			if (ent.getPosition() != null && coord.contains(ent.getPosition())) {
				result.add(ent);
			}
		}

		return result;
	}

	/**
	 * Get every entity on a coord of the wanted type
	 *
	 * @param <T> the class you want to filter.
	 * @param coord the coord where you want to get every entity from
	 * @param filter the class you want to filter.
	 * @return a list with the entitys of the wanted type
	 */
	@SuppressWarnings("unchecked")
	public <T> LinkedList<T> getEntitysOnCoord(final Coordinate coord, final Class<T> filter) {
		LinkedList<T> result = new LinkedList<>();

		for (AbstractEntity ent : entityList) {
			if (ent.hasPosition()
				&& coord.contains(ent.getPosition())//on coordinate?
				&& filter.isInstance(ent)//of type of filter?
			) {
				result.add((T) ent);//add it to list
			}
		}

		return result;
	}

	/**
	 * True if some block has changed in loaded chunks.
	 *
	 * @return returns the modified flag
	 */
	public boolean isModified() {
		return modified;
	}

	@Override
	public Array<Connection<PfNode>> getConnections(PfNode fromNode) {
		return fromNode.getConnections();

	}

	/**
	 *
	 * @param start
	 * @param goal
	 * @return
	 */
	public DefaultGraphPath<PfNode> findPath(Coordinate start, Coordinate goal) {
		IndexedAStarPathFinder<PfNode> pathFinder;
		pathFinder = new IndexedAStarPathFinder<>(this, true);

		DefaultGraphPath<PfNode> path = new DefaultGraphPath<>();
		path.clear();
		Heuristic<PfNode> heuristic = new ManhattanDistanceHeuristic();

		boolean found = pathFinder.searchNodePath(
			new PfNode(start),
			new PfNode(goal),
			heuristic,
			path
		);

		return path;
	}

	@Override
	public int getNodeCount() {
		return Chunk.getBlocksX() * Chunk.getBlocksY();
	}

	/**
	 * check wether a chunk is currently being loaded.
	 * @param chunkX
	 * @param chunkY
	 * @return 
	 */
	public boolean isLoading(int chunkX, int chunkY) {
		for (ChunkLoader lR : loadingRunnables) {
			if (lR.getCoordX() == chunkX && lR.getCoordY() == chunkY) {
				return true;
			}
		}
		return false;
	}
	
	/**
	 * disposes every chunk
	 *
	 * @param save
	 */
	public void dispose(boolean save) {
		for (Chunk chunk : loadedChunks) {
			if (save) {
				chunk.dispose(this, getPath());
			} else {
				chunk.dispose(this, null);
			}
		}
		disposeEntities();
	}

	private static class ManhattanDistanceHeuristic implements Heuristic<PfNode> {

		@Override
		public float estimate(PfNode node, PfNode endNode) {
			return Math.abs(endNode.getX() - node.getX()) + Math.abs(endNode.getY() - node.getY());
		}
	}

	private static class EuklideanDistanceHeuristic implements Heuristic<PfNode> {

		@Override
		public float estimate(PfNode node, PfNode endNode) {
			return node.distanceTo(endNode);
		}
	}
}
