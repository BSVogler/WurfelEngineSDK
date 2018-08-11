/*
 * To change this license header, choose License Headers in Project Properties.
 * To change this template file, choose Tools | Templates
 * and open the template in the editor.
 */
package com.bombinggames.minecrafttowurfelengine;

import com.bombinggames.minecrafttowurfelengine.mcmodify.location.LocChunkInRegion;
import com.bombinggames.minecrafttowurfelengine.mcmodify.location.LocRegionInDimension;
import com.bombinggames.minecrafttowurfelengine.mcmodify.minecraft.Chunk;
import com.bombinggames.minecrafttowurfelengine.mcmodify.minecraft.Dimension;
import com.bombinggames.minecrafttowurfelengine.mcmodify.minecraft.FileRegion;
import com.bombinggames.minecrafttowurfelengine.mcmodify.minecraft.World;
import com.bombinggames.wurfelengine.WE;
import com.bombinggames.wurfelengine.core.map.Generator;
import java.io.File;
import java.io.IOException;
import java.util.HashMap;
import java.util.logging.Level;
import java.util.logging.Logger;

/**
 *
 * @author Benedikt S. Vogler
 */
public class MinecraftLoader implements Generator {

	private final int chunkDim;
	private final int offsetz = 80;
	private final HashMap<Integer, FileRegion> dataRegion;
	private final HashMap<Integer, Chunk> dataChunk;
	private World world;

	int mcRegionDim = 32;//a region contains 32 chunks
	int mcChunkDim = 16;//a chunk has 16*16*256 blocks

	/**
	 * The amount of chunks in memory in one dimension.
	 */
	public MinecraftLoader() {
		chunkDim = WE.getCVars().getValueI("mapIndexSpaceSize");
		dataRegion = new HashMap<>(chunkDim * chunkDim, 0.5f);
		dataChunk = new HashMap<>(chunkDim * chunkDim, 0.5f);
		try {

			File mcSaveLocation = new File("/Users/benediktvogler/Wurfel Engine/mcworld/");
			world = new World(mcSaveLocation);
			world.lock();

//			for (int chunkX = -5; chunkX < 5; chunkX++) {
//				for (int chunkY = -5; chunkY < 5; chunkY++) {
//					int regionX = Math.floorDiv(chunkX, 32);
//					int regionZ = Math.floorDiv(chunkY, 32);
//					FileRegion fileRegion = world.getFileRegion(Dimension.OVERWORLD, new LocRegionInDimension(regionX, regionZ));
//					try {
//						Chunk mcChunk = fileRegion.getChunk(new LocChunkInRegion(-32 * regionX + chunkX, -32 * regionZ + chunkY));
//						if (mcChunk != null) {
//							data.put(chunkX * chunkDim + chunkY, mcChunk);
//						}
//					} catch (IOException ex) {
//						Logger.getLogger(MinecraftImporter.class.getName()).log(Level.SEVERE, null, ex);
//					}
//				}
//			}
		} catch (IOException ex) {
			Logger.getLogger(MinecraftImporter.class.getName()).log(Level.SEVERE, null, ex);
		}
	}

	@Override
	public int generate(int x, int y, int z) {
		//get chunk
		int regionX = Math.floorDiv(x, mcRegionDim*mcChunkDim);
		int regionZ = Math.floorDiv(y, mcRegionDim*mcChunkDim);
		int regionLeft = mcRegionDim*mcChunkDim * regionX;//the block on the leftmost side
		int regionBack = mcRegionDim*mcChunkDim * regionZ;
		int chunkX = Math.floorDiv(x - regionLeft, mcChunkDim);
		int chunkY = Math.floorDiv(y - regionBack, mcChunkDim);
		int chunkLeft = regionLeft + chunkX * mcChunkDim;
		int chunkBack = regionBack + chunkY * mcChunkDim;
		Chunk mcChunk = dataChunk.get(regionX * chunkDim+ regionZ*93246+239242*chunkX+chunkY*245512);
		if (mcChunk == null) {
			FileRegion mcRegion = dataRegion.get(regionX * chunkDim+ regionZ*93246);
			try {
				if (mcRegion==null){
					mcRegion = world.getFileRegion(Dimension.OVERWORLD, new LocRegionInDimension(regionX, regionZ));
					dataRegion.put(regionX * chunkDim+ regionZ*93246, mcRegion);
				}
				mcChunk = mcRegion.getChunk(new LocChunkInRegion(chunkX, chunkY));
			} catch (IOException ex) {
				Logger.getLogger(MinecraftImporter.class.getName()).log(Level.SEVERE, null, ex);
			}
		}
		if (mcChunk != null) {
			dataChunk.put(regionX * chunkDim+ regionZ*93246+239242*chunkX+chunkY*245512, mcChunk);
			short id = mcChunk.BlockID(x - chunkLeft, z + offsetz, y-chunkBack);
			switch (id) {
				case 2:
					return 2;
				case 3:
					return 1;
				case -1:
					return 4;
				default:
					return id;
			}
		}
		return 0;
	}

	@Override
	public void spawnEntities(int x, int y, int z
	) {
	}

}
