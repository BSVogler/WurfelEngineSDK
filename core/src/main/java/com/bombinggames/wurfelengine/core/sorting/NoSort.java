/*
 * If this software is used for a game the official „Wurfel Engine“ logo or its name must be visible in an intro screen or main menu.
 *
 * Copyright 2017 Benedikt Vogler.
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
package com.bombinggames.wurfelengine.core.sorting;

import com.bombinggames.wurfelengine.WE;
import com.bombinggames.wurfelengine.core.Camera;
import com.bombinggames.wurfelengine.core.Controller;
import com.bombinggames.wurfelengine.core.GameView;
import com.bombinggames.wurfelengine.core.gameobjects.AbstractEntity;
import com.bombinggames.wurfelengine.core.gameobjects.AbstractGameObject;
import com.bombinggames.wurfelengine.core.map.Chunk;
import com.bombinggames.wurfelengine.core.map.Iterators.CoveredByCameraIterator;
import com.bombinggames.wurfelengine.core.map.rendering.RenderCell;
import java.util.ArrayList;
import java.util.LinkedList;

/**
 *
 * @author Benedikt Vogler
 */
public class NoSort extends AbstractSorter {

	private final GameView gameView;
	private final CoveredByCameraIterator iterator;

	public NoSort(Camera camera) {
		super(camera);
		gameView = camera.getGameView();
		iterator = new CoveredByCameraIterator(
			gameView.getRenderStorage(),
			camera.getCenterChunkX(),
			camera.getCenterChunkY(),
			0,
			Chunk.getBlocksZ()-1//last layer 
		);
	}

	@Override
	public void createDepthList(LinkedList<AbstractGameObject> depthlist) {
		depthlist.clear();
		int maxsprites = WE.getCVars().getValueI("MaxSprites");
		float renderlimit = gameView.getRenderStorage().getZRenderingLimit();
		//add entitys which should be rendered
		ArrayList<AbstractEntity> ents = Controller.getMap().getEntities();
				
		//this should be made parallel via streams //ents.stream().parallel().forEach(action);?
		int objectsToBeRendered = 0;
		for (AbstractEntity ent : ents) {
			if (ent.hasPosition()
				&& !ent.isHidden()
				&& camera.inViewFrustum(ent.getPosition())
				&& ent.getPosition().getZ() < renderlimit
			) {
				depthlist.add(ent);
				objectsToBeRendered++;
			}
		}

		//iterate over every block in renderstorage
		int topLevel;
		if (gameView.getRenderStorage().getZRenderingLimit() == Float.POSITIVE_INFINITY) {
			topLevel = Chunk.getBlocksZ();
		} else {
			topLevel = (int) (gameView.getRenderStorage().getZRenderingLimit() / RenderCell.GAME_EDGELENGTH);
		}
		iterator.reset(camera.getCenterChunkX(), camera.getCenterChunkY());
		iterator.setTopLimitZ(topLevel-1);
		

		//check/visit every visible cell
		while (iterator.hasNext()) {
			RenderCell cell = iterator.next();
		
			if (
				cell.shouldBeRendered(camera)
				&& cell.getPosition().getZPoint() < renderlimit
				&& objectsToBeRendered < maxsprites
			) {
				//fill only up to available size
				depthlist.add(cell);
				objectsToBeRendered++;
			}
		}
	}
}
