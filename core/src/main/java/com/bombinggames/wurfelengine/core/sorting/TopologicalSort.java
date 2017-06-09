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
import com.bombinggames.wurfelengine.WE;
import com.bombinggames.wurfelengine.core.Camera;
import com.bombinggames.wurfelengine.core.Controller;
import com.bombinggames.wurfelengine.core.Events;
import com.bombinggames.wurfelengine.core.GameView;
import com.bombinggames.wurfelengine.core.gameobjects.AbstractEntity;
import com.bombinggames.wurfelengine.core.gameobjects.AbstractGameObject;
import static com.bombinggames.wurfelengine.core.gameobjects.AbstractGameObject.getSprite;
import com.bombinggames.wurfelengine.core.map.Chunk;
import com.bombinggames.wurfelengine.core.map.Coordinate;
import com.bombinggames.wurfelengine.core.map.Iterators.CoveredByCameraIterator;
import com.bombinggames.wurfelengine.core.map.Point;
import com.bombinggames.wurfelengine.core.map.rendering.GameSpaceSprite;
import com.bombinggames.wurfelengine.core.map.rendering.RenderCell;
import com.bombinggames.wurfelengine.core.map.rendering.RenderChunk;
import java.util.ArrayList;
import java.util.LinkedList;
import java.util.Random;

/**
 *
 * @author Benedikt Vogler
 */
public class TopologicalSort extends AbstractSorter implements Telegraph  {

	private static final long serialVersionUID = 1L;
	public static final float WINDAMPLITUDE = 20f;
	private final static Random RANDOMGENERATOR = new java.util.Random();
	private static float wind;
	private static float windWholeCircle;
	private final GameSpaceSprite gras;
	private static float posXForce;
	private static float posYForce;
	private static float posZForce;
	private static float force = 4000;
	private static float noisenum = 0.1f;
	
	
	public static void updateWind(float dt) {
		windWholeCircle = (windWholeCircle + dt * 0.01f) % WINDAMPLITUDE;
		wind = Math.abs(windWholeCircle - WINDAMPLITUDE / 2)//value between 0 and amp/2
			- WINDAMPLITUDE / 2;//value between -amp/2 and + amp/2
	}
	
	private final float seed;
	private final ArrayList<RenderCell> modifiedCells = new ArrayList<>(30);
	/**
	 * is rendered at the end
	 */
	private final LinkedList<AbstractEntity> renderAppendix = new LinkedList<>();
	private final LinkedList<RenderCell> cacheTopLevel = new LinkedList<>();
	private final GameView gameView;
	private int objectsToBeRendered;
	private int maxsprites;
	
	public TopologicalSort(Camera camera) {
		super(camera);
		gameView = camera.getGameView();
		MessageManager.getInstance().addListener(this, Events.mapChanged.getId());
		gras = new GameSpaceSprite(getSprite('e', (byte) 7, (byte) 0));
		gras.setOrigin(gras.getWidth() / 2f, 0);
		seed = RANDOMGENERATOR.nextFloat();
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
	public void renderSorted() {
		maxsprites = WE.getCVars().getValueI("MaxSprites");


		//add entitys which should be rendered
		ArrayList<AbstractEntity> ents = Controller.getMap().getEntities();
				
		//add entities by inserting them into the render store
		ArrayList<RenderCell> modifiedCells = this.modifiedCells;
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
				RenderCell cell = gameView.getRenderStorage().getCell(ent.getPosition());
				ent.markAsVisitedDS(camera.getId());//so after mark inversion non is visited
				//in the renderstorage no nullpointer should exists, escept object is outside the array
				if (cell == RenderChunk.CELLOUTSIDE) {
					renderAppendix.add(ent);//render at the end
				} else {
					cell.addCoveredEnts(ent);//cell covers entities inside
					modifiedCells.add(cell);
				}
			}
		}
	
		AbstractGameObject.inverseMarkedFlag(camera.getId());
		
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
		//render every entity which has no parent block at the end of the list
		for (AbstractEntity abstractEntity : renderAppendix) {
			abstractEntity.render(gameView);
		}
	}
	
	/**
	 * rebuilds the reference list for fields whihc will be called for the
	 * depthsorting.
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
	 * topological sort for ents
	 * @param o root node
	 */
	private void visit(AbstractEntity o) {
		LinkedList<RenderCell> covered = o.getCoveredBlocks(gameView.getRenderStorage());
		if (!covered.isEmpty()) {
			for (RenderCell m : covered) {
				if (camera.inViewFrustum(m.getPosition())) {
					visit(m);
				}
			}
		}
	}
	
	/**
	 * Topological sort for Cells
	 * @param cell 
	 */
	public void visit(RenderCell cell){
		if (!cell.isMarkedDS(camera.getId())) {
			
			//is a block, so can contain entities
			LinkedList<AbstractEntity> covered = cell.getCoveredEnts();
			
			if (!covered.isEmpty() && !covered.getFirst().isMarkedDS(camera.getId())) {
				covered.getFirst().markAsVisitedDS(camera.getId());
				//inside a cell entities share the dependencies, so only visti first then add all
				visit(covered.getFirst());
					
				for (AbstractEntity e : covered) {
					if (camera.inViewFrustum(e.getPosition())
						&& e.getPosition().getZPoint() < gameView.getRenderStorage().getZRenderingLimit()
						&& objectsToBeRendered < maxsprites//fill only up to available size
					) {
						e.render(gameView);
						objectsToBeRendered++;
					}
				}
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
				cell.render(gameView);
				objectsToBeRendered++;
			}
			
			//draw grass
			if (cell.getId()==0
				&& cell.getCoord().getZ() > 1
				&& Coordinate.getShared().set(cell.getCoord()).add(0, 0, -1).getBlockId() == 1
				&& objectsToBeRendered < maxsprites
			){
				Point pos = cell.getPoint();
				drawGrass(30,pos);
			}
		}
	}
	
	/**
	 * 
	 * @param n
	 * @param pos 
	 */
	public void drawGrass(int n, Point pos){
		for (int i = 0; i < n; i++) {
				//game space
			float xPos = pos.getX();
			float yPos = pos.getY();
			int xOffset = (int) (Math.abs((xPos - seed * 17) * i * (yPos)) % RenderCell.GAME_EDGELENGTH - RenderCell.GAME_EDGELENGTH2);
			int yOffset = (int) (Math.abs(((xPos - i) * 3 * (yPos * seed * 11 - i))) % RenderCell.GAME_EDGELENGTH - RenderCell.GAME_EDGELENGTH2);
			if (Math.abs(xOffset) + Math.abs(yOffset) < RenderCell.GAME_DIAGLENGTH2) {
				gras.setColor(
					1 / 2f,
					1 / 2f - (xOffset + i) % 7 * 0.005f,
					1 / 2f,
					1
				);
				gras.setPosition(
					xPos + xOffset,
					yPos + RenderCell.GAME_DIAGLENGTH2 + yOffset,//there is something wrong with the rendering, so set center to the front
					pos.getZ()
				);

				//wind
				float distanceToForceCenter = (xPos + xOffset - posXForce + 100) * (xPos + xOffset - posXForce + 100)
					+ (-yPos * 2 + yOffset - posYForce + 900) * (-yPos * 2 + yOffset - posYForce + 900);
				float forceRot;
				if (distanceToForceCenter > 200000) {
					forceRot = 0;
				} else {
					forceRot = 600000 / (distanceToForceCenter);
					if (posXForce < xPos) {
						forceRot *= -1;
					}
					if (forceRot > 90) {
						forceRot = 90;
					}
					if (forceRot < -90) {
						forceRot = -90;
					}
				}
				gras.setRotation(i * 0.4f - 10.2f + wind + RANDOMGENERATOR.nextFloat() * noisenum * WINDAMPLITUDE / 2 + forceRot * 0.3f);
				objectsToBeRendered++;
				gras.draw(gameView.getGameSpaceSpriteBatch());
			}
		}
	}

	@Override
	public void createDepthList(LinkedList<AbstractGameObject> depthlist) {
	}

	
}
