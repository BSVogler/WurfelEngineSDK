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

import com.badlogic.gdx.ai.msg.MessageManager;
import com.badlogic.gdx.ai.msg.Telegram;
import com.badlogic.gdx.ai.msg.Telegraph;
import com.bombinggames.wurfelengine.core.Camera;
import com.bombinggames.wurfelengine.core.Events;
import com.bombinggames.wurfelengine.core.GameView;
import com.bombinggames.wurfelengine.core.gameobjects.AbstractGameObject;
import com.bombinggames.wurfelengine.core.map.Chunk;
import com.bombinggames.wurfelengine.core.map.Iterators.CoveredByCameraIterator;
import com.bombinggames.wurfelengine.core.map.rendering.RenderCell;
import java.util.LinkedList;

/**
 * Fills the camera content with blocks and entities into a list and sorts it in
 * the order of the rendering, called the "depthlist". This is done every frame.
 */
public abstract class AbstractSorter implements Telegraph {

	protected final Camera camera;
	protected final LinkedList<RenderCell> iteratorCache = new LinkedList<>();
	protected final GameView gameView;
	private final CoveredByCameraIterator iterator;
	private int lastCenterX;
	private int lastCenterY;
	
	public abstract void createDepthList(LinkedList<AbstractGameObject> depthlist);

	public abstract void renderSorted();

	AbstractSorter(Camera camera) {
		this.camera = camera;
		gameView = camera.getGameView();
		MessageManager.getInstance().addListener(this, Events.mapChanged.getId());
		iterator = new CoveredByCameraIterator(
			gameView.getRenderStorage(),
			camera,
			0,
			getTopLevel() - 1 //last layer
		);
	}

	@Override
	public boolean handleMessage(Telegram msg) {
		if (msg.message == Events.mapChanged.getId()) {
			AbstractSorter.this.bakeIteratorCache();
		}

		return false;
	}
	
	public void updateCacheIfOutdated(){
		int centerChunkX = camera.getCenterChunkX();
		int centerChunkY = camera.getCenterChunkY();
		if (lastCenterX != centerChunkX
			|| lastCenterY != centerChunkY
		) {
			//update the last center
			lastCenterX = centerChunkX;
			lastCenterY = centerChunkY;
			AbstractSorter.this.bakeIteratorCache();
		}
	}

	/**
	 * rebuilds the reference list for fields which will be called for the
	 * depthsorting.
	 *
	 * @param startingLayer
	 */
	public void bakeIteratorCache(int startingLayer) {
		//iterate over every block in renderstorage
		int topLevel;
		if (gameView.getRenderStorage().getZRenderingLimit() == Float.POSITIVE_INFINITY) {
			topLevel = Chunk.getBlocksZ();
		} else {
			topLevel = (int) (gameView.getRenderStorage().getZRenderingLimit() / RenderCell.GAME_EDGELENGTH);
		}
		iterator.reset(camera.getCenterChunkX(), camera.getCenterChunkY());
		iterator.setTopLimitZ(topLevel-1);
		iteratorCache.clear();
		//check/visit every visible cell
		iterator.forEachRemaining(iteratorCache::add);
	}

	public abstract void bakeIteratorCache();

	public int getTopLevel() {
		if (gameView.getRenderStorage().getZRenderingLimit() == Float.POSITIVE_INFINITY) {
			return Chunk.getBlocksZ();
		} else {
			return (int) (gameView.getRenderStorage().getZRenderingLimit() / RenderCell.GAME_EDGELENGTH);
		}
	}

}
