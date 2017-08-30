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
	private CoveredByCameraIterator iterator;
	/**
	 * used to detect the beed fir ab uteratircacge update
	 */
	private int lastCenterX;
	private int lastCenterY;
	private boolean initialized;

	/**
	 * Sorts the list of game objects.
	 *
	 * @param depthlist
	 */
	public abstract void createDepthList(LinkedList<AbstractGameObject> depthlist);

	/**
	 * Renders in sorted order
	 */
	public abstract void renderSorted();

	public AbstractSorter(Camera camera) {
		this.camera = camera;
		gameView = camera.getGameView();
	}

	@Override
	public boolean handleMessage(Telegram msg) {
		//todo when camera is not active the sorter will continue to listen
		if (msg.message == Events.mapChanged.getId() || msg.message == Events.renderStorageChanged.getId()) {
			bakeIteratorCache();
		}

		return false;
	}

	/**
	 * updates the iterator cache. Gets called when the center of the camera
	 * changed.
	 *
	 * @see #bakeIteratorCache()
	 */
	public void updateCacheIfOutdated() {
		if (!initialized
			|| lastCenterX != camera.getCenterChunkX()
			|| lastCenterY != camera.getCenterChunkY()
		) {
			bakeIteratorCache();
		}
	}

	/**
	 * rebuilds the reference list for fields which will be called for the
	 * depthsorting.
	 *
	 */
	protected void bakeIteratorCache() {
		if (iterator == null) {
			iterator = new CoveredByCameraIterator(
				gameView.getRenderStorage(),
				camera,
				0,
				getTopLevel() - 1 //last layer
			);
		}
		initialized = true;
		//update the last center
		lastCenterX = camera.getCenterChunkX();
		lastCenterY = camera.getCenterChunkY();

		//iterate over every block in renderstorage
		iterator.reset(camera.getCenterChunkX(), camera.getCenterChunkY());
		iterator.setTopLimitZ(getTopLevel() - 1);
		iteratorCache.clear();
		//check/visit every visible cell
		iterator.forEachRemaining(iteratorCache::add);
	}

	/**
	 * get the topmost z level which should be rendered
	 *
	 * @return
	 */
	protected int getTopLevel() {
		if (gameView.getRenderStorage().getZRenderingLimit() == Float.POSITIVE_INFINITY) {
			return Chunk.getBlocksZ();
		} else {
			return (int) (gameView.getRenderStorage().getZRenderingLimit() / RenderCell.GAME_EDGELENGTH);
		}
	}
}
