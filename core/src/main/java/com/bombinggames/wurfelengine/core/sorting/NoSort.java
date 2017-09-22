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
import com.bombinggames.wurfelengine.core.map.rendering.RenderCell;
import java.util.ArrayList;
import java.util.LinkedList;

/**
 *
 * @author Benedikt Vogler
 */
public class NoSort extends AbstractSorter {

	private final GameView gameView;
	/**
	 * when set to true will render only block which should be rendered
	 */
	private final boolean filter = true;

	public NoSort(Camera camera) {
		super(camera);
		gameView = camera.getGameView();
	}

	@Override
	public void createDepthList(LinkedList<AbstractGameObject> depthlist) {
		updateCacheIfOutdated();
		depthlist.clear();
		int maxsprites = WE.getCVars().getValueI("MaxSprites");
		float renderlimit = gameView.getRenderStorage().getZRenderingLimit();

		int objectsToBeRendered = 0;

		//check/visit every visible cell
		for (RenderCell cell : iteratorCache) {
			if ((!filter || cell.shouldBeRendered(camera)
				&& cell.getPosition().getZPoint() < renderlimit)
				&& objectsToBeRendered < maxsprites) {//fill only up to available size
				depthlist.add(cell);
				objectsToBeRendered++;
			}
		}

		//add entitys which should be rendered
		ArrayList<AbstractEntity> ents = Controller.getMap().getEntities();
		//this should be made parallel via streams //ents.stream().parallel().forEach(action);?
		for (AbstractEntity ent : ents) {
			if (ent.hasPosition()
				&& !ent.isHidden()
				&& camera.inViewFrustum(ent.getPosition())
				&& ent.getPosition().getZ() < renderlimit) {
				depthlist.add(ent);
				objectsToBeRendered++;
			}
		}
	}

	@Override
	public void renderSorted() {
		updateCacheIfOutdated();
		int maxsprites = WE.getCVars().getValueI("MaxSprites");
		float renderlimit = gameView.getRenderStorage().getZRenderingLimit();

		int objectsToBeRendered = 0;

		//check/visit every visible cell
		if (filter) {
			for (RenderCell cell : iteratorCache) {
				if (cell.shouldBeRendered(camera)
					&& objectsToBeRendered < maxsprites) {
					cell.render(gameView);
					objectsToBeRendered++;
				}
			}
		} else {
			//iteratorCache.stream().limit(maxsprites).parallel().forEach(c -> c.render(gameView));
			for (RenderCell cell : iteratorCache) {
				if (objectsToBeRendered < maxsprites) {
					cell.render(gameView);
					objectsToBeRendered++;
				}
			}
		}

		//add entitys which should be rendered
		ArrayList<AbstractEntity> ents = Controller.getMap().getEntities();
		//this should be made parallel via streams //ents.stream().parallel().forEach(action);?
		for (AbstractEntity ent : ents) {
			if (ent.hasPosition()
				&& !ent.isHidden()
				&& camera.inViewFrustum(ent.getPosition())
				&& ent.getPosition().getZ() < renderlimit) {
				ent.render(gameView);
				objectsToBeRendered++;
			}
		}
	}
}
