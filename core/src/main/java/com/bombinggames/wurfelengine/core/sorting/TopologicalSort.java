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
import com.bombinggames.wurfelengine.core.gameobjects.AbstractEntity;
import com.bombinggames.wurfelengine.core.gameobjects.AbstractGameObject;
import static com.bombinggames.wurfelengine.core.gameobjects.AbstractGameObject.getSprite;
import com.bombinggames.wurfelengine.core.map.Point;
import com.bombinggames.wurfelengine.core.map.rendering.GameSpaceSprite;
import com.bombinggames.wurfelengine.core.map.rendering.RenderCell;
import com.bombinggames.wurfelengine.core.map.rendering.RenderChunk;
import com.bombinggames.wurfelengine.core.map.rendering.RenderStorage;
import static com.bombinggames.wurfelengine.core.sorting.TopoGraphNode.inverseMarkedFlag;
import java.util.ArrayList;
import java.util.LinkedList;
import java.util.Random;

/**
 *
 * @author Benedikt Vogler
 */
public class TopologicalSort extends AbstractSorter {

	private static final long serialVersionUID = 1L;
	public static final float WINDAMPLITUDE = 20f;
	private final static Random RANDOMGENERATOR = new java.util.Random(1);//use the same seed every time
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
	private final ArrayList<TopoGraphNode> modifiedNodes = new ArrayList<>(30);
	/**
	 * is rendered at the end
	 */
	private final LinkedList<AbstractEntity> renderAppendix = new LinkedList<>();
	private int objectsToBeRendered;
	private int maxsprites;
	private final GameSpaceSprite stone;
	
	
	public TopologicalSort(Camera camera) {
		super(camera);
		gras = new GameSpaceSprite(getSprite('e', (byte) 7, (byte) 0));
		stone = new GameSpaceSprite(getSprite('e', (byte) 7, (byte) 1));
		gras.setOrigin(gras.getWidth() / 2f, 0);
		seed = RANDOMGENERATOR.nextFloat();
	}
	
	@Override
	public void renderSorted() {
		updateCacheIfOutdated();
		maxsprites = WE.getCVars().getValueI("MaxSprites");


		//add entitys which should be rendered
		ArrayList<AbstractEntity> ents = Controller.getMap().getEntities();
				
		//add entities by inserting them into the render store
		ArrayList<TopoGraphNode> modifiedNodes = this.modifiedNodes;
		RenderStorage rS = gameView.getRenderStorage();
		modifiedNodes.clear();
		modifiedNodes.ensureCapacity(ents.size());
		LinkedList<AbstractEntity> renderAppendix = this.renderAppendix;
		renderAppendix.clear();
		
		//this should be made parallel via streams //ents.stream().parallel().forEach(action);?
		for (AbstractEntity ent : ents) {
			if (ent.hasPosition()
				&& !ent.isHidden()
				&& camera.inViewFrustum(ent.getPosition())
				&& ent.getPosition().getZ() < rS.getZRenderingLimit()
			) {
				TopoGraphNode node = rS.getCell(ent.getPosition()).getTopoNode();
				ent.markAsVisitedDS(camera.getId());//so after mark inversion non is visited
				//in the renderstorage no nullpointer should exists, except object is outside the array
				if (node == null) {
					renderAppendix.add(ent);//render at the end
				} else {
					node.addCoveredEnts(ent);//cell covers entities inside
					modifiedNodes.add(node);
				}
			}
		}
	
		inverseMarkedFlag(camera.getId());
		
		//iterate over every block in renderstorage
		objectsToBeRendered = 0;
			
		for (RenderCell cell : iteratorCache) {
			if (cell != RenderChunk.CELLOUTSIDE && camera.inViewFrustum(cell.getPosition())) {
				visit(cell.getTopoNode());
			}
		}
		//remove ents from modified blocks
		for (TopoGraphNode modifiedCell : modifiedNodes) {
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
	 * topological sort for ents
	 * @param o root node
	 */
	private void visit(AbstractEntity o) {
		LinkedList<RenderCell> covered = o.getCoveredBlocks(gameView.getRenderStorage());
		if (!covered.isEmpty()) {
			for (RenderCell cell : covered) {
				if (cell.getTopoNode() != null && camera.inViewFrustum(cell.getPosition())) {
					visit(cell.getTopoNode());
				}
			}
		}
	}
	
	/**
	 * Topological sort for Cells
	 * @param node 
	 */
	public void visit(TopoGraphNode node){
		if (!node.isMarkedDS(camera.getId())) {
			
			//is a block, so can contain entities
			LinkedList<AbstractEntity> coveredEnts = node.getCoveredEnts();
			
			if (!coveredEnts.isEmpty() && !coveredEnts.getFirst().isMarkedDS(camera.getId())) {
				coveredEnts.getFirst().markAsVisitedDS(camera.getId());
				//inside a node entities share the dependencies, so only visti first then add all
				visit(coveredEnts.getFirst());
					
				for (AbstractEntity e : coveredEnts) {
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
			node.markAsVisitedDS(camera.getId());

			LinkedList<TopoGraphNode> coveredCells = node.getCoveredBlocks(gameView.getRenderStorage());
			if (!coveredCells.isEmpty()) {
				for (TopoGraphNode n : coveredCells) {
					if (camera.inViewFrustum(n.getCell().getPosition())) {
						visit(n);
					}
				}
			}
			
			
			if (
				node.getCell().shouldBeRendered(camera)
				&& node.getCell().getPosition().getZPoint() < gameView.getRenderStorage().getZRenderingLimit()
				&& objectsToBeRendered < maxsprites
			) {
				//fill only up to available size
				node.getCell().render(gameView);
				objectsToBeRendered++;
	//			//draw grass
	//			if (node.getId()==0
	//				&& node.getCoord().getZ() > 1
	//				&& Coordinate.getShared().set(node.getCoord()).add(0, 0, -1).getBlockId() == 1
	//				&& objectsToBeRendered < maxsprites
	//			){
	//				Point pos = node.getPoint();
	//				drawGrass(10,pos);
	//			}
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
			//is the same each call
			int xOffset = (int) (Math.abs((xPos - seed * 17) * i * (yPos)) % RenderCell.GAME_EDGELENGTH - RenderCell.GAME_EDGELENGTH2);
			int yOffset = (int) (Math.abs(((xPos - i) * 3 * (yPos * seed * 11 - i))) % RenderCell.GAME_EDGELENGTH - RenderCell.GAME_EDGELENGTH2);
			if (Math.abs(xOffset) + Math.abs(yOffset) < RenderCell.GAME_DIAGLENGTH2) {
				GameSpaceSprite sprite;
				if ((xPos+i * yPos * 17) % 7 == 0) {
					sprite = stone;
					sprite.setColor(
						1 / 2f,
						1 / 2f-Math.abs(xOffset*0.2f/RenderCell.GAME_DIAGLENGTH2),
						1 / 2f-Math.abs(xOffset*0.2f/RenderCell.GAME_DIAGLENGTH2),
						1
					);
				} else {
					sprite = gras;
					sprite.setScale(1-xOffset*0.3f/RenderCell.GAME_DIAGLENGTH2);
					sprite.setColor(
						1 / 2f,
						1 / 2f - (xOffset + i) % 7 * 0.005f,
						1 / 2f,
						1
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
					sprite.setRotation(i * 0.4f - 10.2f + wind + RANDOMGENERATOR.nextFloat() * noisenum * WINDAMPLITUDE / 2 + forceRot * 0.3f);
				}
				
				sprite.setPosition(
					xPos + xOffset,
					yPos + RenderCell.GAME_DIAGLENGTH2 + yOffset,//there is something wrong with the rendering, so set center to the front
					pos.getZ()
				);

				objectsToBeRendered++;
				sprite.draw(gameView.getGameSpaceSpriteBatch());
			}
		}
	}

	@Override
	public void createDepthList(LinkedList<AbstractGameObject> depthlist) {
		updateCacheIfOutdated();
		//todo
	}

	@Override
	public void bakeIteratorCache() {
		super.bakeIteratorCache(getTopLevel() - 2);
	}
	
}
