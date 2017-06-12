/*
 * Copyright 2015 Benedikt Vogler.
 * All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions are met:
 *
 * * If this software is used for a game the official „Wurfel Engine“ logo or its name must be
 *   visible in an intro screen or main menu.
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
package com.bombinggames.wurfelengine.core.map;

import com.badlogic.gdx.math.Vector2;
import com.badlogic.gdx.math.Vector3;
import com.bombinggames.wurfelengine.WE;
import com.bombinggames.wurfelengine.core.Camera;
import com.bombinggames.wurfelengine.core.Controller;
import com.bombinggames.wurfelengine.core.GameView;
import com.bombinggames.wurfelengine.core.gameobjects.AbstractEntity;
import com.bombinggames.wurfelengine.core.gameobjects.AbstractGameObject;
import com.bombinggames.wurfelengine.core.gameobjects.Side;
import com.bombinggames.wurfelengine.core.map.rendering.RenderCell;
import com.bombinggames.wurfelengine.core.map.rendering.RenderStorage;
import java.util.ArrayList;
import java.util.LinkedList;
import java.util.function.Predicate;

/**
 * A point is a single position in the game world not bound to the grid. Use
 * this for entities.
 *
 * @author Benedikt Vogler
 * @since WE1.1
 */
public class Point extends Vector3 implements Position {

	private static final long serialVersionUID = 2L;
	private static Point tmp = new Point();

	/**
	 * A shared object to pass values without using the heap.
	 * @return 
	 */
	public static Point getShared() {
		return tmp;
	}

	/**
	 * Creates a point refering to a position in the game world. Points at 0,0,0.
	 */
	public Point() {
	}

	
	/**
	 * Creates a point refering to a position in the game world.
	 *
	 * @param posX The distance from the left border of the map (game space)
	 * @param posY The distance from the top border of the map (game space)
	 * @param height The distance from ground (game space)
	 */
	public Point(float posX, float posY, float height) {
		this.x = posX;
		this.y = posY;
		this.z = height;
	}

	/**
	 *
	 * @param vec
	 */
	public Point(Vector3 vec) {
		this.x = vec.x;
		this.y = vec.y;
		this.z = vec.z;
	}

	/**
	 * Copy-constructor. This constructor copies the values.
	 *
	 * @param point the source of the copy
	 */
	public Point(Point point) {
		this.x = point.x;
		this.y = point.y;
		this.z = point.z;
	}

	/**
	 * Get the height (z-value) of the coordinate.
	 *
	 * @return game dimension
	 */
	public float getZ() {
		return z;
	}

	@Override
	public int getZGrid() {
		return (int) (z / RenderCell.GAME_EDGELENGTH);
	}

	@Override
	public float getZPoint(){
		return z;
	}
	/**
	 *
	 * @param height
	 */
	public void setZ(float height) {
		this.z = height;
	}

	/**
	 * returns coordinate aquivalent. Removes floating of block.<br>
	 * Looks complicated but has runtime O(const). You should avoid this method in loops or the update method because it uses the heap.
	 *
	 * @return coordinate aquivalent, copy safe
	 */
	@Override
	public Coordinate toCoord() {
		//find out where the position is (basic)
		return new Coordinate(
			Math.floorDiv((int) x, RenderCell.GAME_DIAGLENGTH),
			Math.floorDiv((int) y, RenderCell.GAME_DIAGLENGTH) * 2 + 1, //maybe dangerous to optimize code here!
			Math.floorDiv((int) z, RenderCell.GAME_EDGELENGTH)
		).goToNeighbour(Coordinate.getNeighbourSide( //find the specific coordinate (detail)
			x % RenderCell.GAME_DIAGLENGTH,
			y % RenderCell.GAME_DIAGLENGTH
		));
	}

	@Override
	public Coordinate getCoord() {
		return toCoord();
	}

	@Override
	public Point getPoint() {
		return this;
	}
	
	@Override
	public Point toPoint() {
		return this.cpy();
	}
	
	/**
	 * Get the game world position from left
	 *
	 * @return
	 */
	public float getX() {
		return x;
	}

	/**
	 * Get the game world position from top.
	 *
	 * @return
	 */
	public float getY() {
		return y;
	}

	/**
	 * Distance to cell center.
	 * @return the offset to the coordinates center.
	 */
	public float getDistanceToCellCenterX() {
		return x - toCoord().toPoint().x;
	}

	/**
	 * Distance to cell center.
	 * @return the offset to the coordinates center.
	 */
	public float getDistanceToCellCenterY() {
		return y - toCoord().toPoint().y;
	}

	/**
	 * Distance to cell center.
	 * @return the offset to the coordinates center.
	 */
	public float getDistanceToCellCenterZ() {
		return getZ() - getZGrid() * RenderCell.GAME_EDGELENGTH;
	}
	
   @Override
	public byte getBlockId() {
		return (byte) (getBlock()&255);
	}
	
	/**
	 * 
	 * @return 
	 */
	public int getBlock(){
		if (z >= Chunk.getGameHeight()) {
			return 0;
		}
		
		if (z < 0) {
			return (byte) WE.getCVars().getValueI("groundBlockID");
		}

		//bloated in-place code to avoid heap call with toCoord()
		int xCoord = Math.floorDiv((int) x, RenderCell.GAME_DIAGLENGTH);
		int yCoord = Math.floorDiv((int) y, RenderCell.GAME_DIAGLENGTH) * 2 + 1; //maybe dangerous to optimize code here!
		//find the specific coordinate (detail)
		switch (Coordinate.getNeighbourSide(
			x % RenderCell.GAME_DIAGLENGTH,
			y % RenderCell.GAME_DIAGLENGTH
		)) {
			case 0:
				yCoord -= 2;
				break;
			case 1:
				xCoord += yCoord % 2 == 0 ? 0 : 1;
				yCoord--;
				break;
			case 2:
				xCoord++;
				break;
			case 3:
				xCoord += yCoord % 2 == 0 ? 0 : 1;
				yCoord++;
				break;
			case 4:
				yCoord += 2;
				break;
			case 5:
				xCoord -= yCoord % 2 == 0 ? 1 : 0;
				yCoord++;
				break;
			case 6:
				xCoord--;
				break;
			case 7:
				xCoord -= yCoord % 2 == 0 ? 1 : 0;
				yCoord--;
				break;
		}

		return Controller.getMap().getBlock(xCoord, yCoord, (int) (z / RenderCell.GAME_EDGELENGTH));
	}
    
	/**
	 * avoid this method because it creates a new instance.
	 * @return 
	 */
    @Override
    public Point cpy() {
        return new Point(this);
    }

    @Override
    public int getViewSpcX() {
        return (int) (x); //just the position as integer
    }

    @Override
    public int getViewSpcY() {
        return (int)( 
			-y * RenderCell.PROJECTIONFACTORY
            + z * RenderCell.PROJECTIONFACTORZ
		);
    }

	@Override
	public int getProjectionSpaceX(GameView view, Camera camera) {
		return (int) (getViewSpcX() - camera.getViewSpaceX() + camera.getWorldWidthViewport() / 2);// i think zoom and scaling is missing here
	}
	
	@Override
	public int getProjectionSpaceY(GameView view, Camera camera) {
		return (int) (getViewSpcY()-camera.getViewSpaceY() + camera.getWorldHeightViewport() / 2);// i think zoom and scaling is missing here
	}
	
    
	@Override
	public boolean isInMemoryAreaXY() {
		return Controller.getMap().getChunkContaining(this) != null;
	}
	
	@Override
    public boolean isInMemoryAreaXYZ() {
		if (z < 0 || z > Chunk.getGameWidth()){
			return false;
		} else {
			return getBlockId() != 0;
		}
    }

    /**
     *Add a vector to the position
     * @param vector all values in game world values
     * @return returns itself
     */
    public Point add(float[] vector) {
        this.x += vector[0];
        this.y += vector[1];
		this.z += vector[2];
        return this;
    }
    
	/**
     * Add a vector to the position
     * @param vector all values in game world values
     * @return returns itself
     */
    public Point add(Vector2 vector) {
        this.x += vector.x;
        this.y += vector.y;
        return this;
    }
	
     /**
     * Add a vector to the position
     * @param vector all values in game world values
     * @return returns itself
     */
    @Override
    public Point add(Vector3 vector) {
        this.x += vector.x;
        this.y += vector.y;
		this.z += vector.z;
        return this;
    }

    /**
     * Add a vector to the position
     * @param x x value to add
     * @param y y value to add
     * @param z height to add
     * @return returns itself
     */
	@Override
    public Point add(float x, float y, float z) {
        this.x += x;
        this.y += y;
        this.z += z;
        return this;
    }
		
	private int getCoordX(){
		int xCoord = Math.floorDiv((int) x, RenderCell.GAME_DIAGLENGTH);
		int yCoord = Math.floorDiv((int) y, RenderCell.GAME_DIAGLENGTH) * 2 + 1;
		//find the specific coordinate (detail)
		switch (Coordinate.getNeighbourSide(
			x % RenderCell.GAME_DIAGLENGTH,
			y % RenderCell.GAME_DIAGLENGTH
		)) {
			case 0:
				yCoord -= 2;
				break;
			case 1:
				xCoord += yCoord % 2 == 0 ? 0 : 1;
				yCoord--;
				break;
			case 2:
				xCoord++;
				break;
			case 3:
				xCoord += yCoord % 2 == 0 ? 0 : 1;
				yCoord++;
				break;
			case 4:
				yCoord += 2;
				break;
			case 5:
				xCoord -= yCoord % 2 == 0 ? 1 : 0;
				yCoord++;
				break;
			case 6:
				xCoord--;
				break;
			case 7:
				xCoord -= yCoord % 2 == 0 ? 1 : 0;
				yCoord--;
				break;
		}
		
		return xCoord * RenderCell.GAME_DIAGLENGTH + (y % 2 != 0 ? RenderCell.VIEW_WIDTH2 : 0)+RenderCell.GAME_DIAGLENGTH2;
	}
	
	private int getCoordY() {
		int yCoord = Math.floorDiv((int) y, RenderCell.GAME_DIAGLENGTH) * 2 + 1;
		//find the specific coordinate (detail)
		switch (Coordinate.getNeighbourSide(
			x % RenderCell.GAME_DIAGLENGTH,
			y % RenderCell.GAME_DIAGLENGTH
		)) {
			case 0:
				yCoord -= 2;
				break;
			case 1:
				yCoord--;
				break;
			case 3:
				yCoord++;
				break;
			case 4:
				yCoord += 2;
				break;
			case 5:
				yCoord++;
				break;
			case 6:
				break;
			case 7:
				yCoord--;
				break;
		}

		return yCoord * RenderCell.GAME_DIAGLENGTH2 + RenderCell.GAME_DIAGLENGTH2;
	}
	
	/**
	 * Relative to the current coordiante field set the offset.
	 *
	 * @return 
	 */
	public Point setToCenterOfCell() {
		//code belo is optimized version of "return this.toCoord().toPoint()"
		int xCoord = Math.floorDiv((int) x, RenderCell.GAME_DIAGLENGTH);
		int yCoord = Math.floorDiv((int) y, RenderCell.GAME_DIAGLENGTH) * 2 + 1;
		//find the specific coordinate (detail)
		switch (Coordinate.getNeighbourSide(
			x % RenderCell.GAME_DIAGLENGTH,
			y % RenderCell.GAME_DIAGLENGTH
		)) {
			case 0:
				yCoord -= 2;
				break;
			case 1:
				xCoord += yCoord % 2 == 0 ? 0 : 1;
				yCoord--;
				break;
			case 2:
				xCoord++;
				break;
			case 3:
				xCoord += yCoord % 2 == 0 ? 0 : 1;
				yCoord++;
				break;
			case 4:
				yCoord += 2;
				break;
			case 5:
				xCoord -= yCoord % 2 == 0 ? 1 : 0;
				yCoord++;
				break;
			case 6:
				xCoord--;
				break;
			case 7:
				xCoord -= yCoord % 2 == 0 ? 1 : 0;
				yCoord--;
				break;
		}
		
		this.x = xCoord * RenderCell.GAME_DIAGLENGTH + (yCoord % 2 != 0 ? RenderCell.VIEW_WIDTH2 : 0);
		this.y = yCoord * RenderCell.GAME_DIAGLENGTH2;
		this.z = (int) (z / RenderCell.GAME_EDGELENGTH) * RenderCell.GAME_EDGELENGTH;
		return this;
	}

    /**
     * Trace a ray through the map until ray hits non air block.<br>
     * Does not work properly with the staggered map but is quite fast.
     * @param dir dir of the ray
     * @param maxDistance the distance after which it should stop. (in game meters)
	 * @param rs
	 * @param hitCondition
     * @return can return <i>null</i> if not hitting anything. The normal on the back sides may be wrong. The normals are in a turned coordiante system.
     * @since 1.2.29
	 * @see #rayMarching(com.badlogic.gdx.math.Vector3, float, com.bombinggames.wurfelengine.core.map.rendering.RenderStorage, java.util.function.Predicate) 
     */
	public Intersection raycast(final Vector3 dir, float maxDistance, final RenderStorage rs, final Predicate<Byte> hitCondition) {
		/*  Call the callback with (x,y,z,value,normal) of all blocks along the line
		segment from point 'origin' in vector dir 'dir' of length
		'maxDistance'. 'maxDistance' may be infinite.
		'normal' is the normal vector of the normal of that block that was entered.
		It should not be used after the callback returns.
		If the callback returns a true value, the traversal will be stopped.
		 */
		// From "A Fast Voxel Traversal Algorithm for Ray Tracing"
		// by John Amanatides and Andrew Woo, 1987
		// <http://www.cse.yorku.ca/~amana/research/grid.pdf>
		// <http://citeseer.ist.psu.edu/viewdoc/summary?doi=10.1.1.42.3443>
		// Extensions to the described algorithm:
		//   • Imposed a distance limit.
		//   • The normal passed through to reach the current cube is provided to
		//     the callback.
		// The foundation of this algorithm is a parameterized representation of
		// the provided ray,
		//                    origin + t * dir,
		// except that t is not actually stored; rather, at any given point in the
		// traversal, we keep track of the *greater* t values which we would have
		// if we took a step sufficient to cross a cube boundary along that axis
		// (i.e. change the integer part of the coordinate) in the variables
		// tMaxX, tMaxY, and tMaxZ.
		// Cube containing origin point.
		
		// Avoids an infinite loop.
        if (dir.isZero()) {
			throw new Error("Raycast in zero direction!");
		}
		dir.cpy().nor();
		
		Coordinate isectC = toCoord();
		Point isectPtmp = Point.getShared();
		//curent coordinate position
        int curX = isectC.getX();
        int curY = isectC.getY();
        int curZ = isectC.getZ();
		
		// Direction to increment x,y,z when stepping.
		int stepX = (int) Math.signum(dir.x);
		int stepY = (int) Math.signum(dir.y);
		int stepZ = (int) Math.signum(dir.z);
        // See description above. The initial values depend on the fractional
		// part of the origin.
		float tMaxX = intbound(x, dir.x);
		float tMaxY = intbound(y, dir.y);
		float tMaxZ = intbound(z, dir.z);
        // The change in t when taking a step (always positive).
        float tDeltaX = stepX / dir.x;
		float tDeltaY = stepY / dir.y;
		float tDeltaZ = stepZ / dir.z;

		/* while ray has not gone past bounds of world. can be outside when will enter */
		while (
			(stepZ > 0 ? curZ < Chunk.getBlocksZ(): curZ >= 0)
			&& isectC.isInMemoryAreaXY()
		) {
			//update intersection coordinate
			isectC.set(curX, curY, curZ);
			//intersect?
			if (
				rs == null
				||
				(curZ*RenderCell.GAME_EDGELENGTH < rs.getZRenderingLimit() && !rs.isClipped(isectC))
			) {
				byte id = Controller.getMap().getBlockId(isectC);
				if (
					id != 0
					&& (hitCondition == null || hitCondition.test(id))
				){
					//found intersection point
					isectPtmp.setFromCoord(isectC);
					if (distanceTo(isectPtmp) <= maxDistance*RenderCell.GAME_EDGELENGTH)
						return Intersection.intersect(isectC, this, dir);
					else return null;
				}
			}

            /*tMaxX stores the t-value at which we cross a cube boundary along the
             X axis, and similarly for Y and Z. Therefore, choosing the least tMax
            chooses the closest cube boundary. Only the first case of the four
            has been commented in detail.*/
            if (tMaxX < tMaxY) {
                if (tMaxX < tMaxZ) {
                    if (tMaxX > maxDistance) break;
                    // Update which cube we are now in.
                    curX += stepX;
                    // Adjust tMaxX to the next X-oriented boundary crossing.
                    tMaxX += tDeltaX;
                } else {
                    if (tMaxZ > maxDistance) break;
                    curZ += stepZ;
                    tMaxZ += tDeltaZ;
                }
            } else {
                if (tMaxY < tMaxZ) {
                    if (tMaxY > maxDistance) break;
                    curY += stepY;
                    tMaxY += tDeltaY;
                } else {
                  // Identical to the second case, repeated for simplicity in
                  // the conditionals.
                  if (tMaxZ > maxDistance) break;
                  curZ += stepZ;
                  tMaxZ += tDeltaZ;
                }
            }
        }
        //ground hit
        if (curZ <= 0) {
			Point intersectpoint = new Point(
				curX * RenderCell.GAME_DIAGLENGTH + (curY % 2 != 0 ? RenderCell.VIEW_WIDTH2 : 0),
				curY * RenderCell.GAME_DIAGLENGTH2,
				0
			);
			float distance = this.distanceTo(intersectpoint);
			if (distance <= RenderCell.GAME_EDGELENGTH * maxDistance) {
				return new Intersection(intersectpoint, Side.TOP, distance);
			} else {
				return null;
			}
		} else {
			return null;
		}
    }

    /**
     * Find the smallest positive t such that s+t*ds is an integer.
     * @param s
     * @param ds
     * @return 
     * @since 1.2.29
     */
	private int intbound(float s, float ds) {
		if (ds < 0) {
			return intbound(-s, -ds);
		} else {
			s = (s % 1 + 1) % 1;//modulo
			// problem is now s+t*ds = 1
			return (int) ((1 - s) / ds);
		}
	}

	/**
	 * Sends a ray by moving a coordinate though the map. Slow but it mostly returns precise results.<br>
	 * Stops at first point where the criteria is met.
	 * 
	 * @param dir
	 * @param maxDistance game space in meters
	 * @param rS used when regarding clipping information
	 * @param hitCondition can be null
	 * @return intersection point
	 * @see #raycast(com.badlogic.gdx.math.Vector3, float, com.bombinggames.wurfelengine.core.map.rendering.RenderStorage, java.util.function.Predicate) 
	 */
	public Intersection rayMarching(
		final Vector3 dir,
		float maxDistance,
		final RenderStorage rS,
		final Predicate<Byte> hitCondition
	){
		if (dir == null) {
			throw new NullPointerException("Direction of raycasting not defined");
		}
		
		if (dir.isZero()) {
			throw new Error("Raycast in zero direction!");
		}
		
		Point traverseP = cpy();
		dir.cpy().nor().scl(3);
		Coordinate isectC = traverseP.toCoord();
		int lastCoordZ = 0;
		while (
			(lastCoordZ > 0
			&& lastCoordZ < Chunk.getBlocksZ()
			|| isectC.isInMemoryAreaXY())
			&& distanceToSquared(traverseP) < maxDistance*RenderCell.GAME_EDGELENGTH*maxDistance*RenderCell.GAME_EDGELENGTH
		){
			//move
			traverseP.add(dir);
			isectC.setFromPoint(traverseP);
			lastCoordZ = isectC.getZ();
			
			if (
				rS == null
				||
				(lastCoordZ*RenderCell.GAME_EDGELENGTH < rS.getZRenderingLimit() && !rS.isClipped(isectC))
			) {
				//evaluate hit
				byte id = isectC.getBlockId();
				if (
					id != 0
					&& (hitCondition == null || hitCondition.test(id))
				){
					Intersection interse = new Intersection(traverseP, null, distanceTo(traverseP));
					interse.calcNormal(traverseP);
					return interse;
				}
			}
		}
		//check for ground hit
		if (traverseP.getZ() <= 0) {
			traverseP.setZ(0);//clamp at 0
			float distance = this.distanceTo(traverseP);
			if (distance <= RenderCell.GAME_EDGELENGTH * maxDistance) {
				return new Intersection(traverseP, Side.TOP, distance);
			} else {
				return null;
			}
		} else {
			return null;
		}
	}
		
	/**
	 *
	 * @param pos
	 * @return the distance from this point to the other point in game world
	 * coordinates
	 */
	@Override
	public float distanceTo(Position pos) {
		return distanceTo(pos.getPoint());
	}

	/**
	 *
	 * @param point
	 * @return the distance from this point to the other point in game
	 * coordinates
	 */
	public float distanceTo(Point point) {
		float dX = x - point.x;
		float dY = y - point.y;
		float dZ = z - point.z;
		return (float) Math.sqrt(dX * dX + dY * dY + dZ * dZ);
	}

	/**
	 *
	 * @param object
	 * @return the distance from this point to the other object
	 */
	@Override
	public float distanceTo(AbstractGameObject object) {
		return distanceTo(object.getPoint());
	}

	@Override
	public float distanceToSquared(AbstractGameObject object) {
		return distanceToSquared(object.getPoint());
	}

	@Override
	public float distanceToSquared(Position pos) {
		return distanceToSquared(pos.getPoint());
	}
	
	/**
	 * The result is squared for fast comparison.
	 * @param point
	 * @return the distance from this point to the other point in game
	 * coordinates squared
	 */
	public float distanceToSquared(Point point) {
		float dX = x - point.x;
		float dY = y - point.y;
		float dZ = z - point.z;
		return dX * dX + dY * dY + dZ * dZ;
	}
	
	/**
	 * checks only x and y.
	 *
	 * @param point
	 * @return the distance from this point to the other point only regarding
	 * horizontal components.
	 */
	public float distanceToHorizontal(Point point) {
		float dX = x - point.x;
		float dY = y - point.y;
		return (float) Math.sqrt(dX * dX + dY * dY);
	}
	
	/**
	 * checks only x and y.
	 *
	 * @param point
	 * @return the distance from this point to the other point only regarding
	 * horizontal components.
	 */
	public float distanceToHorizontalSquared(Point point) {
		float dX = x - point.x;
		float dY = y - point.y;
		return dX * dX + dY * dY;
	}

	/**
	 * checks only x and y.
	 *
	 * @param object
	 * @return the distance from this point to the other point only regarding
	 * horizontal components.
	 */
	@Override
	public float distanceToHorizontal(AbstractGameObject object) {
		return distanceToHorizontal(object.getPoint());
	}

	@Override
	public float distanceToHorizontal(Position pos) {
		return distanceToHorizontal(pos.getPoint());
	}
	
		/**
	 * checks only x and y.
	 *
	 * @param object
	 * @return the distance from this point to the other point only regarding
	 * horizontal components.
	 */
	@Override
	public float distanceToHorizontalSquared(AbstractGameObject object) {
		return distanceToHorizontalSquared(object.getPoint());
	}

	@Override
	public float distanceToHorizontalSquared(Position pos) {
		return distanceToHorizontalSquared(pos.getPoint());
	}

	/**
	 * get entities in radius
	 *
	 * @param radius in game dimension pixels
	 * @return every entitie in radius
	 */
	public ArrayList<AbstractEntity> getEntitiesNearby(float radius) {
		ArrayList<AbstractEntity> result = new ArrayList<>(5);//defautl size 5

		for (AbstractEntity entity : Controller.getMap().getEntities()) {
			if (entity.hasPosition() && distanceTo(entity.getPoint()) < radius) {
				result.add(entity);
			}
		}

		return result;
	}
	
	@Override
	public LinkedList<AbstractEntity> getEntitiesNearbyHorizontal(float radius) {
		LinkedList<AbstractEntity> result = new LinkedList<>();
		ArrayList<AbstractEntity> entityList = Controller.getMap().getEntities();
		for (AbstractEntity entity : entityList) {
			if (distanceToHorizontal(entity.getPoint()) < radius) {
				result.add(entity);
			}
		}

		return result;
	}

	@SuppressWarnings("unchecked")
	@Override
	public <T> LinkedList<T> getEntitiesNearbyHorizontal(float radius, final Class<T> type) {
		LinkedList<T> result = new LinkedList<>();
		ArrayList<AbstractEntity> entityList = Controller.getMap().getEntities();

        for (AbstractEntity entity : entityList) {//check every entity
            if (
				entity.hasPosition()
				&& type.isInstance(entity) //if the entity is of the wanted type
				&& distanceToHorizontalSquared(entity.getPoint()) < radius*radius
			) {
                result.add((T) entity);//add it to list
            }
        }

        return result;
	}
	
	@Override
	public int getChunkX() {
		return Math.floorDiv((int) x, Chunk.getGameWidth());
	}

	@Override
	public int getChunkY() {
		return Math.floorDiv((int) y, Chunk.getGameDepth());
	}

	/**
	 *
	 * @param p
	 * @param maxdistance game space in meters
	 * @return
	 */
	public boolean canSee(Point p, float maxdistance) {
		Vector3 vecToTarget = p.cpy().sub(this).nor();
		//check if can see target
		Intersection intersect = p.rayMarching(
			vecToTarget,
			maxdistance,
			null,
			(Byte t) -> !RenderCell.isTransparent(t,(byte)0)
		);
		return !(intersect != null
			&& distanceTo(intersect.getPoint()) < distanceTo(p)//check if point is before
			);
	}

	/**
	 * overwrites the coordinates with values from another point. Faster then
	 * creating a new object-
	 *
	 * @param point is not modified
	 * @return itself for chaining
	 */
	public Point set(final Point point) {
		this.x = point.x;
		this.y = point.y;
		this.z = point.z;
		return this;
	}

	@Override
	public <T> LinkedList<T> getEntitiesNearby(float radius, Class<T> type) {
		LinkedList<T> result = new LinkedList<>();//default size 5
		LinkedList<T> entities = Controller.getMap().getEntitys(type);
		for (T entity : entities) {
			if (distanceTo(((AbstractEntity)entity).getPosition()) < radius) {
				result.add(entity);
			}
		}

		return result;
	}

	@Override
	public Chunk getChunk() {
		return Controller.getMap().getChunkContaining(toCoord());
	}

	/**
	 * Set x,y,z based on a coordinate.
	 * @param coord 
	 * @return  
	 */
	public Point setFromCoord(final Coordinate coord) {
		x = coord.getX() * RenderCell.GAME_DIAGLENGTH + (coord.getY() % 2 != 0 ? RenderCell.VIEW_WIDTH2 : 0);
		y = coord.getY() * RenderCell.GAME_DIAGLENGTH2;
		z = coord.getZ() * RenderCell.GAME_EDGELENGTH;
		return this;
	}

	/**
	 *
	 * @return
	 */
	public boolean isObstacle() {
		return RenderCell.isObstacle(getBlockId());
	}

}
