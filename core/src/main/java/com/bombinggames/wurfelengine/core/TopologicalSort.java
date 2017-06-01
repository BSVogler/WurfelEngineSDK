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
package com.bombinggames.wurfelengine.core;

import com.badlogic.gdx.ai.msg.MessageManager;
import com.badlogic.gdx.ai.msg.Telegram;
import com.badlogic.gdx.ai.msg.Telegraph;
import com.bombinggames.wurfelengine.WE;
import com.bombinggames.wurfelengine.core.gameobjects.AbstractEntity;
import com.bombinggames.wurfelengine.core.gameobjects.AbstractGameObject;
import com.bombinggames.wurfelengine.core.gameobjects.Renderable;
import com.bombinggames.wurfelengine.core.map.Chunk;
import com.bombinggames.wurfelengine.core.map.Iterators.CoveredByCameraIterator;
import com.bombinggames.wurfelengine.core.map.rendering.RenderCell;
import com.bombinggames.wurfelengine.core.map.rendering.RenderChunk;
import java.util.ArrayList;
import java.util.LinkedList;

/**
 *
 * @author Benedikt Vogler
 */
public class TopologicalSort extends AbstractSorter implements Telegraph  {

	private final ArrayList<RenderCell> modifiedCells = new ArrayList<>(30);
	private final ArrayList<AbstractEntity> entsInCells = new ArrayList<>(30);
	/**
	 * is rendered at the end
	 */
	private final LinkedList<AbstractEntity> renderAppendix = new LinkedList<>();
	private final LinkedList<RenderCell> cacheTopLevel = new LinkedList<>();
	private final GameView gameView;
	private LinkedList<Renderable> depthlist;
	private int objectsToBeRendered;
	private int maxsprites;
	
	public TopologicalSort(Camera camera) {
		super(camera);
		gameView = camera.getGameView();
		MessageManager.getInstance().addListener(this, Events.mapChanged.getId());
	}
	
	@Override
	public boolean handleMessage(Telegram msg) {
		if (msg.message == Events.mapChanged.getId()) {
			rebuildTopLevelCache();
			return true;
		}
		
		return false;
	}
	
	
	@Override
	void  createDepthList(LinkedList<Renderable> depthlist) {
		this.depthlist = depthlist;
		depthlist.clear();
		maxsprites = WE.getCVars().getValueI("MaxSprites");

		//inverse dirty flag
		AbstractGameObject.inverseMarkedFlag(camera.getId());
		
		//add entitys which should be rendered
		ArrayList<AbstractEntity> ents = Controller.getMap().getEntities();
				
		//add entities by inserting them into the render store
		ArrayList<RenderCell> modifiedCells = this.modifiedCells;
		ArrayList<AbstractEntity> entsInCells = this.entsInCells;
		entsInCells.clear();
		entsInCells.ensureCapacity(ents.size());
		modifiedCells.clear();
		modifiedCells.ensureCapacity(ents.size());
		LinkedList<AbstractEntity> renderAppendix = this.renderAppendix;
		renderAppendix.clear();
		
		//this should be made parallel via streams //ents.stream().parallel().forEach(action);?
		for (AbstractEntity ent : ents) {
			if (ent.hasPosition()
				&& !ent.isHidden()
				&& camera.inViewFrustum(ent.getPosition())
				&& ent.getPosition().getZ() < gameView.getRenderStorage().getZRenderingLimit()
			) {
				RenderCell cellAbove = gameView.getRenderStorage().getCell(ent.getPosition());
				//in the renderstorage no nullpointer should exists, escept object is outside the array
				if (cellAbove == RenderChunk.CELLOUTSIDE) {
					renderAppendix.add(ent);//render at the end
				} else {
					cellAbove.addCoveredEnts(ent);//cell covers entities inside
					modifiedCells.add(cellAbove);
					entsInCells.add(ent);
				}
			}
		}
	
		//iterate over every block in renderstorage
		objectsToBeRendered = 0;
			
		for (RenderCell cell : cacheTopLevel) {
			if (cell != RenderChunk.CELLOUTSIDE && camera.inViewFrustum(cell.getPosition())) {
				visit(cell);
		}
			}
		//remove ents from modified blocks
		for (RenderCell modifiedCell : modifiedCells) {
			modifiedCell.clearCoveredEnts();
		}
		
		//sort by depth
		renderAppendix.sort((AbstractGameObject o1, AbstractGameObject o2) -> {
			float d1 = o1.getDepth();
			float d2 = o2.getDepth();
			if (d1 > d2) {
				return 1;
			} else {
				if (d1 == d2) {
					return 0;
				}
				return -1;
			}
		});
		depthlist.addAll(renderAppendix);//render every entity which has no parent block at the end of the list
	}
	
	/**
	 * rebuilds the reference list for fields whihc will be called for the depthsorting.
	 */
	public void rebuildTopLevelCache() {
		int topLevel;
		if (gameView.getRenderStorage().getZRenderingLimit() == Float.POSITIVE_INFINITY) {
			topLevel = Chunk.getBlocksZ();
		} else {
			topLevel = (int) (gameView.getRenderStorage().getZRenderingLimit() / RenderCell.GAME_EDGELENGTH);
		}
		CoveredByCameraIterator iterator = new CoveredByCameraIterator(
			gameView.getRenderStorage(),
			camera.getCenterChunkX(),
			camera.getCenterChunkY(),
			topLevel-2,
			topLevel-1//last layer 
		);
		cacheTopLevel.clear();
		//check/visit every visible cell
		while (iterator.hasNext()) {
			RenderCell cell = iterator.next();
			cacheTopLevel.add(cell);
		}
	}
	
	/**
	 * topological sort
	 * @param o root node
	 */
	private void visit(AbstractEntity o) {
		if (!o.isMarkedDS(camera.getId())) {
			o.markAsVisitedDS(camera.getId());
			LinkedList<RenderCell> covered = o.getCoveredBlocks(gameView.getRenderStorage());
			if (!covered.isEmpty()) {
				for (RenderCell m : covered) {
					if (camera.inViewFrustum(m.getPosition())) {
						visit(m);
					}
				}
			}

			if (
				o.shouldBeRendered(camera)
				&& o.getPosition().getZPoint() < gameView.getRenderStorage().getZRenderingLimit()
				&& objectsToBeRendered < maxsprites
			) {
				//fill only up to available size
				depthlist.add(o);
				objectsToBeRendered++;
			}
		}
	}
	
	/**
	 * 
	 * @param cell 
	 */
	public void visit(RenderCell cell){
		if (!cell.isMarkedDS(camera.getId())) {
			
			//is a block
			LinkedList<AbstractEntity> covered = cell.getCoveredEnts();

			boolean injectEnt = false;
			for (AbstractEntity m : covered) {//entities share graph in a cell, could be otimized here
				if (!m.isMarkedDS(camera.getId())) {
					injectEnt = true;
				}
				if (camera.inViewFrustum(m.getPosition())) {
					visit(m);
				}
			}

			if (injectEnt) {
				return;
			}

			//continue regularly
			cell.markAsVisitedDS(camera.getId());

			LinkedList<RenderCell> coveredBlocks = cell.getCoveredBlocks(gameView.getRenderStorage());
			if (!coveredBlocks.isEmpty()) {
				for (RenderCell m : coveredBlocks) {
					if (camera.inViewFrustum(m.getPosition())) {
						visit(m);
					}
				}
			}
			
			
			if (
				cell.shouldBeRendered(camera)
				&& cell.getPosition().getZPoint() < gameView.getRenderStorage().getZRenderingLimit()
				&& objectsToBeRendered < maxsprites
			) {
				//fill only up to available size
				depthlist.add(cell);
				objectsToBeRendered++;
			}
		}
	}

	
}
