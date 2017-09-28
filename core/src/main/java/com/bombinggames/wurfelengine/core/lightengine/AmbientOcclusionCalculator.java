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
package com.bombinggames.wurfelengine.core.lightengine;

import com.bombinggames.wurfelengine.core.map.Chunk;
import com.bombinggames.wurfelengine.core.map.Coordinate;
import com.bombinggames.wurfelengine.core.map.CustomBlocks;
import com.bombinggames.wurfelengine.core.map.Iterators.DataIterator3D;
import com.bombinggames.wurfelengine.core.map.Map;
import com.bombinggames.wurfelengine.core.map.rendering.RenderCell;
import com.bombinggames.wurfelengine.core.map.rendering.RenderChunk;
import com.bombinggames.wurfelengine.core.map.rendering.RenderStorage;

/**
 *
 * @author Benedikt Vogler
 */
public class AmbientOcclusionCalculator {

	/**
	 * calcualtes the ambient occlusion for a chunk.
	 *
	 * @param rS the renderStorage where this calculation is performed
	 * @param chunk
	 */
	public static void calcAO(RenderStorage rS, RenderChunk chunk) {
		if (chunk==null) throw new IllegalArgumentException("Chunk can not be null.");
		//iterate over every block in chunk
		Coordinate coord = new Coordinate(0, 0, 0);
		DataIterator3D<RenderCell> iterator = chunk.getIterator(0, Chunk.getBlocksZ() - 1);
		CustomBlocks config = Map.getBlockConfig();
		
		//Algorithm runs similar to marching cubes, could be optimized to cache neighbor row at the side and below todo
		while (iterator.hasNext()) {
			RenderCell next = iterator.next();
			//skip air and blocks without sides
			if (next.hasSides()) {
				//analyze top side
				coord = coord.set(
					chunk.getTopLeftCoordinateX() + iterator.getCurrentIndex()[0],
					chunk.getTopLeftCoordinateY() + iterator.getCurrentIndex()[1],
					iterator.getCurrentIndex()[2] + 1
				);

				int aoFlags = 0;
				//first check 0,2,4,6 then check 1,3,5,7
				for (int side = 0; side < 9; side += 2) {//first round even sides
					//second round odd sides
					if (side == 8) {
						side = 1;
					}
					RenderCell neighBlock = coord.goToNeighbour(side).getRenderCell(rS);
					byte neighborId = neighBlock.getSpriteId();
					byte neighborValue = neighBlock.getSpriteValue();
					if (neighborId != 0 && !config.isTransparent(neighborId, neighborValue) && config.hasSides(neighborId, neighborValue)) {
						aoFlags |= 1 << (side + 8);
						//don't double draw the sides in between
						if (side % 2 == 1) {
							aoFlags &= ~(1 << (((side + 1) % 8) + 8));//set next to false
							aoFlags &= ~(1 << (((side + 7) % 8) + 8));//Set previous to false
						}
					} else {
						aoFlags &= ~(1 << (side + 8));
					}
					coord.goToNeighbour((side+4) % 8);//go back to center
				}
				
				//analyze right side of block, side id 2
				//check right half, which is equivalent to top right at pos 1
				coord = coord.set(
					chunk.getTopLeftCoordinateX() + iterator.getCurrentIndex()[0],
					chunk.getTopLeftCoordinateY() + iterator.getCurrentIndex()[1],
					iterator.getCurrentIndex()[2]
				);//get current coordinate

				//left side, side 0
				//right corner
				RenderCell neighBlock = coord.add(0, 2, -1).getRenderCell(rS);
				byte neighborId = neighBlock.getSpriteId();
				byte neighborValue = neighBlock.getSpriteValue();
				if (!config.isTransparent(neighborId, neighborValue)&& config.hasSides(neighborId, neighborValue)) {
					aoFlags |= 1 << 3;//first byte position 3
				}
				coord.add(0, -2, 1);//revert

				//check bottom left
				neighBlock = coord.add(-1, 0, -1).getRenderCell(rS);
				neighborId = neighBlock.getSpriteId();
				neighborValue = neighBlock.getSpriteValue();
				if (neighborId != 0 && !config.isTransparent(neighborId, neighborValue) && config.hasSides(neighborId, neighborValue)) {
					aoFlags |= 1 << 5;//first byte position 5
				}
				coord.add(1, 0, 1);

				//check left half, which is equivalent to top right at pos 7
				neighBlock = coord.add(-1, 0, 0).getRenderCell(rS);//go to left
				neighborId = neighBlock.getSpriteId();
				neighborValue = neighBlock.getSpriteValue();
				if (neighborId != 0 && !config.isTransparent(neighborId, neighborValue) && config.hasSides(neighborId, neighborValue)) {
					aoFlags |= 1 << 6;//first byte position 6
					aoFlags &= ~(1 << 5);//set next to false
					aoFlags &= ~(1 << 7);//Set previous to false
				}
				coord.add(1, 0, 0);//revert

				//check bottom side, which is equivalent ot top right at pos 5
				neighBlock = coord.add(0, 0, -1).goToNeighbour(5).getRenderCell(rS);//revert changes and go to neighbor
				neighborId = neighBlock.getSpriteId();
				neighborValue = neighBlock.getSpriteValue();
				if (neighborId != 0 && !config.isTransparent(neighborId, neighborValue) && config.hasSides(neighborId, neighborValue)) {
					aoFlags |= 1 << 4;//first byte position 4
					aoFlags &= ~(1 << 5);//set next to false
					aoFlags &= ~(1 << 3);//Set previous to false
				}
				coord.goToNeighbour(1).add(0, 0, 1);//revert

				//right side, side 2
				//check bottom left
				neighBlock =coord.add(1, 0, -1).getRenderCell(rS);
				neighborId = neighBlock.getSpriteId();
				neighborValue = neighBlock.getSpriteValue();
				if (neighborId != 0 && !config.isTransparent(neighborId, neighborValue) && config.hasSides(neighborId, neighborValue)) {
					aoFlags |= 1 << 19;//third byte position 3
				}
				coord.add(-1, 0, 1);

				//check left corner
				neighBlock = coord.add(0, 2, -1).getRenderCell(rS);//revert changes and go to neighbor
				neighborId = neighBlock.getSpriteId();
				neighborValue = neighBlock.getSpriteValue();
				if (neighborId != 0 && !config.isTransparent(neighborId, neighborValue) && config.hasSides(neighborId, neighborValue)) {
					aoFlags |= 1 << 21;//third byte position 5
				}
				coord.add(0, -2, 1);

				//right
				neighBlock = coord.add(1, 0, 0).getRenderCell(rS);
				neighborId = neighBlock.getSpriteId();
				neighborValue = neighBlock.getSpriteValue();
				if (neighborId != 0 && !config.isTransparent(neighborId, neighborValue) && config.hasSides(neighborId, neighborValue)) {
					aoFlags |= 1 << 18;//third byte position 2
					aoFlags &= ~(1 << 17);//set next to false
					aoFlags &= ~(1 << 19);//Set previous to false
				}
				coord.add(-1, 0, 0);

				//check bottom side, which is equivalent to top right at pos 3
				neighBlock = coord.add(0, 0, -1).goToNeighbour(3).getRenderCell(rS);//revert changes and go to neighbor
				neighborId = neighBlock.getSpriteId();
				neighborValue = neighBlock.getSpriteValue();
				if (neighborId != 0 && !config.isTransparent(neighborId, neighborValue) && config.hasSides(neighborId, neighborValue)) {
					aoFlags |= 1 << 20;//third byte position 4
					aoFlags &= ~(1 << 21);//set next to false
					aoFlags &= ~(1 << 19);//Set previous to false
				}
				coord.goToNeighbour(7).add(0, 0, 1);

				neighBlock = coord.add(0, 2, 0).getRenderCell(rS);//revert changes and go to neighbor
				neighborId = neighBlock.getSpriteId();
				neighborValue = neighBlock.getSpriteValue();
				if (neighborId != 0 && !config.isTransparent(neighborId, neighborValue) && config.hasSides(neighborId, neighborValue)) {
					aoFlags |= 1 << 2;//first byte position 2
					aoFlags |= 1 << 22;//third byte position 6
				}
				next.setAoFlags(aoFlags);
			}
		}
	}

}
