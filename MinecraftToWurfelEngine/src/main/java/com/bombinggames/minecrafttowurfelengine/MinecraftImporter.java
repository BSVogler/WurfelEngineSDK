package com.bombinggames.minecrafttowurfelengine;

import com.bombinggames.minecrafttowurfelengine.mcmodify.location.LocChunkInRegion;
import com.bombinggames.minecrafttowurfelengine.mcmodify.location.LocRegionInDimension;
import com.bombinggames.minecrafttowurfelengine.mcmodify.minecraft.Chunk;
import com.bombinggames.minecrafttowurfelengine.mcmodify.minecraft.Dimension;
import com.bombinggames.minecrafttowurfelengine.mcmodify.minecraft.FileRegion;
import com.bombinggames.minecrafttowurfelengine.mcmodify.minecraft.World;
import com.bombinggames.wurfelengine.WE;
import com.bombinggames.wurfelengine.core.Controller;
import com.bombinggames.wurfelengine.core.GameView;
import com.bombinggames.wurfelengine.core.WorkingDirectory;
import com.bombinggames.wurfelengine.core.loading.LoadingScreen;
import com.bombinggames.wurfelengine.core.map.Coordinate;
import com.bombinggames.wurfelengine.core.map.Map;
import com.bombinggames.wurfelengine.extension.basicmainmenu.GameViewWithCamera;
import java.io.File;
import java.io.IOException;
import java.util.logging.Level;
import java.util.logging.Logger;

/**
 *
 * @author Benedikt S. Vogler
 */
public class MinecraftImporter {

	static int mcRegionDim = 32;//a region contains 32 chunks
	static int mcChunkDim = 16;//a chunk has 16*16*256 blocks
	
	/**
	 * @param args the command line arguments
	 */	
	public static void main(String[] args) {
		Map.setDefaultGenerator(new MinecraftLoader());
		//WE.getCVars().get(cvar);

		WE.addPostLaunchCommands(() -> {
			//loadBySetting();
			try {
				File weSaveLocation = new File(WorkingDirectory.getMapsFolder().getAbsolutePath() + "/mcworld/");
				Map wemap = new Map(weSaveLocation, 0);
				//wemap.getCVars().get("chunkBlocksZ").setValue(16);
				//wemap.getCVars().get("chunkBlocksX").setValue(16);
				wemap.getCVars().get("chunkBlocksY").setValue(16);
				Controller.setMap(wemap);
				Controller c = new Controller();
				GameView v = new GameViewWithCamera();
				WE.initAndStartGame(new LoadingScreen(), c, v);
			} catch (IOException ex) {
				Logger.getLogger(MinecraftImporter.class.getName()).log(Level.SEVERE, null, ex);
			}
		});
//WE.addPostLaunchCommands(() -> {
//			loadBySetting();
//		});
		WE.launch("mapimport", args);
		System.exit(0);
	}

	static void loadBySetting() {
		try {
			File weSaveLocation = new File(WorkingDirectory.getMapsFolder().getAbsolutePath() + "/mcworld/");
			Map wemap = new Map(weSaveLocation, 0);
			wemap.getCVars().get("chunkBlocksZ").setValue(16);
			wemap.getCVars().get("chunkBlocksX").setValue(16);
			wemap.getCVars().get("chunkBlocksY").setValue(16);
			Controller.setMap(wemap);
			
			File mcSaveLocation = new File("/Users/benediktvogler/Wurfel Engine/mcworld/");
			World world = new World(mcSaveLocation);
			world.lock();
			for (int chunkXDir = -5; chunkXDir < 5; chunkXDir++) {
				for (int chunkYDir = -5; chunkYDir < 5; chunkYDir++) {
					int regionX = Math.floorDiv(chunkXDir, mcRegionDim );
					int regionZ = Math.floorDiv(chunkYDir, mcRegionDim );
					FileRegion fileRegion = world.getFileRegion(Dimension.OVERWORLD, new LocRegionInDimension(regionX, regionZ));
					int regionLeft = mcRegionDim * regionX;//the block on the leftmost side
					int regionBack = mcRegionDim * regionZ;
					int chunkX = Math.floorDiv(x - regionLeft, mcChunkDim);
					int chunkY = Math.floorDiv(y - regionBack, mcChunkDim);
					try {
						Chunk mcChunk = fileRegion.getChunk(new LocChunkInRegion(-32 * regionX + chunkX, -32 * regionZ + chunkY));
						//WE.launch("mapimport", new String[]{"--headless"});
						int offsetz = 69;
						for (int wex = chunkX * 16; wex < chunkX * 16 + 16; wex++) {//minecraft has 16*16
							for (int wey = chunkY * 16; wey < chunkY * 16 + 16; wey++) {
								for (int wez = offsetz; wez < com.bombinggames.wurfelengine.core.map.Chunk.getBlocksZ() + offsetz; wez++) { //255 height
									short id = mcChunk.BlockID(wex, wez, wey);
									//System.out.println(id);
									if (id > 0) {
										wemap.setBlock(new Coordinate(wex, wey, wez - offsetz), (byte) (id == 2 ? 1 : id == 3 ? 2 : id), (byte) 0);
									}
								}
							}
						}
					} catch (IOException ex) {
						Logger.getLogger(MinecraftImporter.class.getName()).log(Level.SEVERE, null, ex);
					}
				}
			}
			
			Controller c = new Controller();
			GameView v = new GameViewWithCamera();
			WE.initAndStartGame(new LoadingScreen(), c, v);
		} catch (IOException ex) {
			Logger.getLogger(MinecraftImporter.class.getName()).log(Level.SEVERE, null, ex);
		}
	}
}
