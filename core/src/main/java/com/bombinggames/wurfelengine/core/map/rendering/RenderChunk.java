/*
 * If this software is used for a game the official „Wurfel Engine“ logo or its name must be visible in an intro screen or main menu.
 *
 * Copyright 2016 Benedikt Vogler.
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
package com.bombinggames.wurfelengine.core.map.rendering;

import com.badlogic.gdx.utils.Pool;
import com.bombinggames.wurfelengine.core.gameobjects.Side;
import com.bombinggames.wurfelengine.core.map.Chunk;
import com.bombinggames.wurfelengine.core.map.Coordinate;
import com.bombinggames.wurfelengine.core.map.Iterators.DataIterator3D;

/**
 * Stores display data for a {@link Chunk}. <br> <br>
 * This may is outdated: <br>
 * If a cell contains no block/air then the shared {@link #CELLOUTSIDE} is used.
 * @author Benedikt Vogler
 */
public class RenderChunk {
	
	/**
	 * In theory if in a cell is no data available use this block. Uses air internally.<br>
	 * block-by-block differences must not be used because this is a shared
	 * object. Topological Depth Sort needs individual containers. If the topolgogical
	 * sort would be removed from the engine this could be again be used.
	 */
	public static final RenderCell CELLOUTSIDE = RenderCell.newRenderCell((byte) 0, (byte) 0);
	/**
	 * a pool containing chunkdata
	 */
	private static final Pool<RenderCell[][][]> DATAPOOL;
	
	static {
		DATAPOOL = new Pool<RenderCell[][][]>(3) {
			@Override
			protected RenderCell[][][] newObject() {
				//bigger by two cells because of an overlap
				RenderCell[][][] arr = new RenderCell[Chunk.getBlocksX()][Chunk.getBlocksY()][Chunk.getBlocksZ()];
				for (RenderCell[][] x : arr) {
					for (RenderCell[] y : x) {
						for (int z = 0; z < y.length; z++) {
							y[z] = RenderCell.newRenderCell((byte) 0, (byte) 0);//nullpointerobject or just a null pointer can be used, as the rendering process expects an individiual container at each cell
						}
					}
				}
				return arr;
			}
		};
	}
	
	/**
	 *clears the pool to free memory
	 */
	public static void clearPool(){
		DATAPOOL.clear();
	}
	
	/**
	 * chunk used for rendering with this object
	 */
	private final Chunk chunk;
	
	/**
	 * the actual data stored in this renderchunk. Can not contain null
	 */
	private final RenderCell data[][][];
	private boolean cameraAccess;

	/**
	 * With init
	 *
	 * @param chunk linked chunk which is then rendered
	 */
	public RenderChunk(Chunk chunk) {
		data = DATAPOOL.obtain();
		this.chunk = chunk;
		initData();
	}

	/**
	 * fills every render cell based on the data from the map
	 *
	 */
	public void initData() {
		int tlX = chunk.getTopLeftCoordinateX();
		int tlY = chunk.getTopLeftCoordinateY();

		//fill every data cell
		int blocksX = Chunk.getBlocksX();
		int blocksY = Chunk.getBlocksY();
		int blocksZ = Chunk.getBlocksZ();
		for (int xInd = 0; xInd < blocksX; xInd++) {
			for (int yInd = 0; yInd < blocksY; yInd++) {
				for (int z = 0; z < blocksZ; z++) {
					//update only if cell changed
					int blockAtPos = chunk.getBlockByIndex(xInd, yInd, z);//get block from map
					if ((blockAtPos & 255) != data[xInd][yInd][z].getId()) {
						data[xInd][yInd][z] = RenderCell.newRenderCell((byte) (blockAtPos & 255), (byte) ((blockAtPos >> 8) & 255));
					}
					
					//set the coordinate
					data[xInd][yInd][z].getPosition().set(
						tlX + xInd,
						tlY + yInd,
						z
					);
					data[xInd][yInd][z].setUnclipped();
					resetShadingFor(xInd, yInd, z);
				}
			}
		}
	}

	/**
	 *
	 * @param coord only coordinates which are in this chunk
	 * @return 
	 */
	public RenderCell getCell(Coordinate coord) {
		if (coord.getZ() >= Chunk.getBlocksZ()) {
			return CELLOUTSIDE;
		}
		return data[coord.getX() - chunk.getTopLeftCoordinateX()][coord.getY() - chunk.getTopLeftCoordinateY()][coord.getZ()];
	}
	
	/**
	 *
	 * @param x coordinate, must be contained in this chunk
	 * @param y coordinate, must be contained in this chunk
	 * @param z coordinate, must be contained in this chunk
	 * @return
	 */
	public RenderCell getCell(int x, int y, int z) {
		//if is above (outside) container
		if (z >= Chunk.getBlocksZ()) {
			return CELLOUTSIDE;
		}
		return data[x - chunk.getTopLeftCoordinateX()][y - chunk.getTopLeftCoordinateY()][z];
	}

	/**
	 * get the pointer to the data
	 * @return 
	 */
	public RenderCell[][][] getData() {
		return data;
	}

	/**
	 * Resets the clipping for every block.
	 */
	protected void resetClipping() {
		int blocksZ = Chunk.getBlocksZ();
		int blocksX = Chunk.getBlocksX();
		int blocksY = Chunk.getBlocksY();
		for (int x = 0; x < blocksX; x++) {
			for (int y = 0; y < blocksY; y++) {
				for (int z = 0; z < blocksZ; z++) {
					data[x][y][z].setUnclipped();
				}
			}
		}
	}

	/**
	 * Resets the shading for one block. Calculates drop shadow from blocks
	 * above.
	 *
	 * @param idexX index pos
	 * @param idexY index pos
	 * @param idexZ index pos
	 */
	public void resetShadingFor(int idexX, int idexY, int idexZ) {
		int blocksZ = Chunk.getBlocksZ();
		if (idexZ < Chunk.getBlocksZ() && idexZ >= 0) {
			RenderCell block = data[idexX][idexY][idexZ];
			if (block != null) {
				data[idexX][idexY][idexZ].setLightlevel(1);

				//check if block above is transparent
				if (idexZ < blocksZ - 2
					&& (data[idexX][idexY][idexZ + 1].isTransparent())
				) {
					//two cells above is a block casting shadows
					if (!data[idexX][idexY][idexZ + 2].isTransparent()
					) {
						data[idexX][idexY][idexZ].setLightlevel(0.8f, Side.TOP);
					//three blocks above is one
					} else if (idexZ < blocksZ - 3
						&& (data[idexX][idexY][idexZ + 2].isTransparent())
						&& !data[idexX][idexY][idexZ + 3].isTransparent()
					) {
						data[idexX][idexY][idexZ].setLightlevel(0.92f, Side.TOP);
					}
				}
			}
		}
	}

	/**
	 *
	 * @return
	 */
	public int getTopLeftCoordinateX() {
		return chunk.getTopLeftCoordinateX();
	}

	/**
	 *
	 * @return
	 */
	public int getTopLeftCoordinateY() {
		return chunk.getTopLeftCoordinateY();
	}

	/**
	 * Returns an iterator which iterates over the data in this chunk.
	 *
	 * @param startingZ
	 * @param limitZ the last layer (including).
	 * @return
	 */
	public DataIterator3D<RenderCell> getIterator(final int startingZ, final int limitZ) {
		return new DataIterator3D<>(
			data,
			startingZ,
			limitZ
		);
	}

	/**
	 *
	 * @see Chunk#getChunkX() 
	 * @return
	 */
	public int getChunkX() {
		return chunk.getChunkX();
	}

	/**
	 *
	 * @see Chunk#getChunkY() 
	 * @return
	 */
	public int getChunkY() {
		return chunk.getChunkY();
	}

	/**
	 *
	 * @param x
	 * @param y
	 * @param z
	 * @return
	 */
	protected RenderCell getCellByIndex(int x, int y, int z) {
		return data[x][y][z];
	}

	/**
	 * If not used can be removed.
	 * @return true if a camera rendered this chunk this frame. 
	 */
	protected boolean getCameraAccess() {
		return cameraAccess;
	}

	/**
	 * Camera used this chunk this frame?
	 *
	 * @param b
	 */
	protected void setCameraAccess(boolean b) {
		cameraAccess = b;
	}

	/**
	 *
	 */
	protected void dispose() {
		DATAPOOL.free(data);
	}

}
