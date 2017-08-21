/*
 * Copyright 2013 Benedikt Vogler.
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
 * * Neither the name of Bombing Games nor Benedikt Vogler nor the names of its contributors 
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

import com.badlogic.gdx.Gdx;
import com.badlogic.gdx.graphics.Color;
import com.badlogic.gdx.graphics.g2d.Sprite;
import com.badlogic.gdx.graphics.g2d.TextureAtlas.AtlasRegion;
import com.bombinggames.wurfelengine.core.Camera;
import com.bombinggames.wurfelengine.core.Controller;
import com.bombinggames.wurfelengine.core.GameView;
import com.bombinggames.wurfelengine.core.gameobjects.AbstractEntity;
import com.bombinggames.wurfelengine.core.gameobjects.AbstractGameObject;
import com.bombinggames.wurfelengine.core.gameobjects.Side;
import com.bombinggames.wurfelengine.core.gameobjects.SimpleEntity;
import com.bombinggames.wurfelengine.core.map.Coordinate;
import com.bombinggames.wurfelengine.core.map.CustomBlocks;
import com.bombinggames.wurfelengine.core.map.Point;
import com.bombinggames.wurfelengine.core.map.Position;
import com.bombinggames.wurfelengine.core.sorting.TopologicalSort;
import java.util.LinkedList;

/**
 * Something which can be rendered and therefore saves render information shared
 * across cameras. A RenderCell should not use the event system. The class
 * extends/wraps the plain data of the block with a position and
 * {@link AbstractGameObject} class methods. The wrapped cell is not
 * referenced.<br>
 * The block id in teh map can have different id then used for rendering. The
 * rendering sprite id's are set in the constructor or later manualy.<br>
 *
 * @author Benedikt Vogler
 */
public class RenderCell extends AbstractGameObject {
    private static final long serialVersionUID = 1L;
	/**
	 * indexed acces to spritesheet {id}{value}{side}
	 */
    private static AtlasRegion[][][] blocksprites = new AtlasRegion[RenderCell.OBJECTTYPESNUM][RenderCell.VALUESNUM][3];
	
    /**
     * a list where a representing color of the block is stored
     */
    private static final Color[][] COLORLIST = new Color[RenderCell.OBJECTTYPESNUM][RenderCell.VALUESNUM];
	private static boolean staticShade;
	/**
	 * frame number of last rebuild
	 */
	private static long rebuildCoverList = 0;
	private static SimpleEntity destruct = new SimpleEntity((byte) 3,(byte) 0);
	private static Color tmpColor = new Color();
	
	/**
	 * Screen depth of a block/object sprite in pixels. This is the length from
	 * the top to the middle border of the block.
	 */
	public transient static final int VIEW_DEPTH = 100;
	/**
	 * The half (1/2) of VIEW_DEPTH. The short form of: VIEW_DEPTH/2
	 */
	public transient static final int VIEW_DEPTH2 = VIEW_DEPTH / 2;
	/**
	 * A quarter (1/4) of VIEW_DEPTH. The short form of: VIEW_DEPTH/4
	 */
	public transient static final int VIEW_DEPTH4 = VIEW_DEPTH / 4;

	/**
	 * The width (x-axis) of the sprite size.
	 */
	public transient static final int VIEW_WIDTH = 200;
	/**
	 * The half (1/2) of VIEW_WIDTH. The short form of: VIEW_WIDTH/2
	 */
	public transient static final int VIEW_WIDTH2 = VIEW_WIDTH / 2;
	/**
	 * A quarter (1/4) of VIEW_WIDTH. The short form of: VIEW_WIDTH/4
	 */
	public transient static final int VIEW_WIDTH4 = VIEW_WIDTH / 4;

	/**
	 * The height (y-axis) of the sprite size.
	 */
	public transient static final int VIEW_HEIGHT = 122;
	/**
	 * The half (1/2) of VIEW_HEIGHT. The short form of: VIEW_WIDTH/2
	 */
	public transient static final int VIEW_HEIGHT2 = VIEW_HEIGHT / 2;
	/**
	 * A quarter (1/4) of VIEW_HEIGHT. The short form of: VIEW_WIDTH/4
	 */
	public transient static final int VIEW_HEIGHT4 = VIEW_HEIGHT / 4;

	/**
	 * The game space dimension size's aequivalent to VIEW_DEPTH or VIEW_WIDTH.
	 * Because the x axis is not shortened those two are equal.
	 */
	public transient static final int GAME_DIAGLENGTH = VIEW_WIDTH;

	/**
	 * Half (1/2) of GAME_DIAGLENGTH.
	 */
	public transient static final int GAME_DIAGLENGTH2 = VIEW_WIDTH2;

	/**
	 * Pixels per game spaces meter (edge length).<br>
	 * 1 game meter ^= 1 GAME_EDGELENGTH<br>
	 */
	public transient static final int GAME_EDGELENGTH = (int) (GAME_DIAGLENGTH / 1.41421356237309504880168872420969807856967187537694807317667973799f);

	/**
	 * Half (1/2) of GAME_EDGELENGTH.
	 */
	public transient static final int GAME_EDGELENGTH2 = GAME_EDGELENGTH / 2;

	/**
	 * The factor by what the Z axis is distorted when game to view projection
	 * is applied. Usually not 1 because of the angle of projection.
	 */
	public transient static final float PROJECTIONFACTORZ = VIEW_HEIGHT / (float) GAME_EDGELENGTH;
	
	/**
	 * The factor by what the Y axis is distorted when game to view projection
	 * is applied. Usually not 1 because of the angle of projection.
	 */
	public transient static final float PROJECTIONFACTORY = VIEW_DEPTH / (float) GAME_DIAGLENGTH;

	/**
	 * the max. amount of different object types
	 */
	public transient static final int OBJECTTYPESNUM = 124;
	/**
	 * the max. amount of different values
	 */
	public transient static final int VALUESNUM = 64;

	/**
	 * the factory for custom blocks
	 */
	private static CustomBlocks customBlocks;
	
	/**
	 * If you want to define custom id's &gt;39
	 *
	 * @param customBlockFactory new value of customBlockFactory
	 */
	public static void setCustomBlockFactory(CustomBlocks customBlockFactory) {
		customBlocks = customBlockFactory;
	}

	/**
	 *
	 * @return
	 */
	public static CustomBlocks getFactory() {
		return customBlocks;
	}

	/**
	 * value between 0-100
	 *
	 * @param coord
	 * @param health
	 */
//	public static void setHealth(Coordinate coord, byte id, byte value, byte health) {
//		if (customBlocks != null) {
//			customBlocks.onSetHealth(coord, health, id, value);
//		}
//		if (health <= 0 && !isIndestructible(id, value)) {
//			//make an invalid air instance (should be null)
//			this.id = 0;
//			this.value = 0;
//		}
//	}

	/**
	 * The health is stored in a byte in the range [0;100]
	 *
	 * @param block
	 * @return
	 */
	public static byte getHealth(int block) {
		return (byte) ((block >> 16) & 255);
	}

	/**
	 * creates a new RenderCell instance based on the data
	 *
	 * @param id
	 * @param value
	 * @return
	 */
	public static RenderCell newInstance(byte id, byte value) {
		if (id == 0 || id == 4) {//air and invisible wall
			RenderCell a = new RenderCell(id, value);
			a.setHidden(true);
			return a;
		}

		if (id == 9) {
			return new Sea(id, value);
		}

		if (customBlocks != null) {
			return customBlocks.toRenderBlock(id, value);
		} else {
			return new RenderCell(id, value);
		}
	}

	/**
	 * 
	 * @param id
	 * @param value
	 * @return 
	 */
	public static boolean isObstacle(byte id, byte value) {
		if (id > 9 && customBlocks != null) {
			return customBlocks.isObstacle(id, value);
		}
		if (id == 9) {
			return false;
		}
		
		return id != 0;
	}
	
	/**
	 * 
	 * @param block
	 * @return 
	 */
	public static boolean isObstacle(int block) {
		return isObstacle((byte)(block&255), (byte)((block>>8)&255));
	}
	
	/**
	 * 
	 * When it is possible to see though the sides.
	 * @param spriteId
	 * @param spriteValue
	 * @return 
	 */
	public static boolean isTransparent(byte spriteId, byte spriteValue) {
		if (spriteId==0 || spriteId == 9 || spriteId == 4) {
			return true;
		}
		
		if (spriteId > 9 && customBlocks != null) {
			return customBlocks.isTransparent(spriteId, spriteValue);
		}
		return false;
	}
	
	/**
	 * 
	 * @param spriteIdValue id and value in one int
	 * @return 
	 */
	public static boolean isTransparent(int spriteIdValue) {
		return isTransparent((byte)(spriteIdValue&255), (byte)((spriteIdValue>>8)&255));
	}

	/**
	 * Check if the block is liquid.
	 *
	 * @param id
	 * @param value
	 * @return true if liquid, false if not
	 */
	public static boolean isLiquid(byte id, byte value) {
		if (id > 9 && customBlocks != null) {
			return customBlocks.isLiquid(id, value);
		}
		return id == 9;
	}
	
		/**
	 * Check if the block is liquid.
	 *
	 * @param block first byte id, second value, third health
	 * @return true if liquid, false if not
	 */
	public static boolean isLiquid(int block) {
		return isLiquid((byte)(block&255), (byte)((block>>8)&255));
	}
	
	/**
	 *
	 * @param id
	 * @param value
	 * @return
	 */
	public static boolean isIndestructible(byte id, byte value) {
		if (customBlocks != null) {
			return customBlocks.isIndestructible(id, value);
		}
		return false;
	}

	/**
	 * get the name of a combination of id and value
	 *
	 * @param id
	 * @param value
	 * @return
	 */
	public static String getName(byte id, byte value) {
		if (id < 10) {
			switch (id) {
				case 0:
					return "air";
				case 1:
					return "grass";
				case 2:
					return "dirt";
				case 3:
					return "stone";
				case 4:
					return "invisible obstacle";
				case 8:
					return "sand";
				case 9:
					return "water";
				default:
					return "undefined";
			}
		} else {
			if (customBlocks != null) {
				return customBlocks.getName(id, value);
			} else {
				return "undefined";
			}
		}
	}

	/**
	 *
	 * @param spriteId
	 * @param spriteValue
	 * @return
	 */
	public static boolean hasSides(byte spriteId, byte spriteValue) {
		if (spriteId == 0 || spriteId == 4) {
			return false;
		}
		
		if (spriteId > 9 && customBlocks != null) {
			return customBlocks.hasSides(spriteId, spriteValue);
		}
		return true;
	}
	
	/**
	 * Indicate whether the blocks should get shaded independent of the light engine by default.
	 * @param shade 
	 */
	public static void setStaticShade(boolean shade){
		staticShade = shade;
	}
	
	/**
	 * Returns a sprite sprite of a specific side of the block
	 *
	 * @param id the id of the block
	 * @param value the value of teh block
	 * @param side Which side?
	 * @return an sprite of the side
	 */
	public static AtlasRegion getBlockSprite(final byte id, final byte value, final Side side) {
		if (getSpritesheet() == null) {
			throw new NullPointerException("No spritesheet found. Load with #loadSheet()");
		}

		if (blocksprites[id][value][side.getCode()] != null) { //load if not already loaded
			return blocksprites[id][value][side.getCode()];
		} else {
			AtlasRegion sprite = getSpritesheet().findRegion('b' + Byte.toString(id) + "-" + value + "-" + side.getCode());
			if (sprite == null) {
				Gdx.app.debug("debug", 'b' + Byte.toString(id) + "-" + value + "-" + side.getCode() + " not found");
				//if there is no sprite show the default "sprite not found sprite" for this category
				sprite = getSpritesheet().findRegion("b0-0-" + side.getCode());

				if (sprite == null) {//load generic error sprite if category sprite failed
					sprite = getSpritesheet().findRegion("error");
					if (sprite == null) {
						throw new NullPointerException("Sprite and category error not found and even the generic error sprite could not be found. Something with the sprites is fucked up.");
					}
				}
			}
			blocksprites[id][value][side.getCode()] = sprite;
			return sprite;
		}
	}
	
	/**
	 * checks if a sprite is defined. if not the error sprite will be rendered
	 * @param spriteId
	 * @param spriteValue
	 * @return 
	 */
	public static boolean isSpriteDefined(byte spriteId, byte spriteValue) {
		return spriteId != 0
			&& getSpritesheet() != null
			&& getSpritesheet().findRegion('b' + Byte.toString(spriteId) + "-" + spriteValue + "-0" + (RenderCell.hasSides(spriteId, spriteValue) ? "-0" : "")) != null;
	}
	
	/**
	 * Only relevant to topological depth sort {@link TopologicalSort}. Sets a flag which causes the baking of the coverlist. This causes every field wich contains the covered neighbors to be rebuild. Used to prenvent duplicate graph rebuilds in one frame.
	 */
	public static void flagRebuildCoverList() {
		RenderCell.rebuildCoverList = Gdx.graphics.getFrameId();
	}

   /**
     * Returns a color representing the block. Picks from the sprite sprite.
     * @param id id of the RenderCell
     * @param value the value of the block.
     * @return copy of a color representing the block
     */
    public static Color getRepresentingColor(final byte id, final byte value){
        if (COLORLIST[id][value] == null){ //if not in list, add it to the list
            COLORLIST[id][value] = new Color();
            int colorInt;
            
            if (RenderCell.hasSides(id, value)){//if has sides, take top block    
                AtlasRegion texture = getBlockSprite(id, value, Side.TOP);
                if (texture == null) return new Color();
                colorInt = getPixmap().getPixel(
                    texture.getRegionX()+VIEW_DEPTH2, texture.getRegionY()+VIEW_DEPTH4);
            } else {
                AtlasRegion texture = getSprite('b', id, value);
                if (texture == null) return new Color();
                colorInt = getPixmap().getPixel(
                    texture.getRegionX()+VIEW_DEPTH2, texture.getRegionY()+VIEW_DEPTH2);
            }
            Color.rgba8888ToColor(COLORLIST[id][value], colorInt);
            return COLORLIST[id][value].cpy(); 
        } else return COLORLIST[id][value].cpy(); //return value when in list
    }
	
	/**
     *
     * @return
     */
    public static AtlasRegion[][][] getBlocksprites() {
        return blocksprites;
    }
	
    /**
     * dipsose the static fields
     */
    public static void staticDispose(){
		//clear index
		for (AtlasRegion[][] blocksprite : blocksprites) {
			for (AtlasRegion[] atlasRegions : blocksprite) {
				for (int i = 0; i < atlasRegions.length; i++) {
					atlasRegions[i] = null;
				}
			}
		}
    }

	/**
	 * final because an id change must go though {@link #newInstance(byte, byte) }
	 */
	private final byte id;
	/**
	 * sprite value
	 */
	private byte value;
	private Coordinate coord = new Coordinate(0, 0, 0);
	
	//view data
	/**
	 * Each side has four RGB101010 colors (each edge) with a each 10bit float
	 * per color channel. The channel brightness is obtained by dividing bits by
	 * fraction /2^10-1 = 1023. Each field index describes a vertex (edge) 0-3. Vertex start at left, then top, then right.
	 */
	private final int[] color = new int[3*4];//three sides with four edges
	/**
	 * byte 0: left side, byte 1: top side, byte 2: right side.<br>In each byte the
	 * bit order: <br>
	 * &nbsp;&nbsp;\&nbsp;0/<br>
	 * 7&nbsp;&nbsp;\/1<br>
	 * \&nbsp;&nbsp;/\&nbsp;&nbsp;/<br>
	 * 6\/8&nbsp;\/2<br>
	 * &nbsp;/\&nbsp;&nbsp;/\<br>
	 * /&nbsp;&nbsp;\/&nbsp;3\<br>
	 * &nbsp;&nbsp;5/\<br>
	 * &nbsp;&nbsp;/&nbsp;4\<br>
	 * <br>
	 **/
	private int aoFlags;
	/**
	 * Three bits used, for each side one. byte position equals side id. TODO: move to aoFlags byte #3
	 */
	private byte clipping;
	/**
	 * Stores references to neighbor blocks which are covered. For topological sort.
	 */
	private final LinkedList<RenderCell> covered = new LinkedList<>();
	/**
	 * for topological sort. At the end contains both entities and blocks
	 */
	private final LinkedList<AbstractEntity> coveredEnts = new LinkedList<>();
	private transient GameSpaceSprite side3;
	private transient GameSpaceSprite side2;
	/**
	 * frame number to avoid multiple calculations in one frame
	 */
	private long lastRebuild;
	
	/**
	 * For direct creation. You should use the factory method instead.
	 * @param id 
	 * @see #newInstance(byte, byte) 
	 */
    public RenderCell(byte id){
        super();
		this.id = id;
	}
	
	/**
	 * For direct creation. You should use the factory method instead.
	 * @param id
	 * @param value 
	 * @see #newInstance(byte, byte) 
	 */
	public RenderCell(byte id, byte value){
		super();
		this.id = id;
		this.value = value;
	}
	
	/**
	 * game logic value. Sprite Id may differ.
	 * @return 
	 * @see #getSpriteId() 
	 */
	public byte getId() {
		return id;
	}

	/**
	 * game logic value. Sprite value may differ.
	 * @return 
	 * @see #getSpriteValue()
	 */
	public byte getValue() {
		return value;
	}
	
	/**
	 *
	 * @return
	 */
	public boolean isObstacle() {
		return RenderCell.isObstacle(id, value);
	}

    @Override
    public String getName() {
        return RenderCell.getName(id, value);
    }

	@Override
	public Point getPoint() {
		return Point.getShared().setFromCoord(coord);
	}

	@Override
	public Coordinate getCoord() {
		return coord;
	}
	
	@Override
	public int getDimensionZ() {
		return RenderCell.GAME_EDGELENGTH;
	}

	@Override
	public final Coordinate getPosition() {
		return coord;
	}

	@Override
	public void setPosition(Position pos) {
		coord = pos.toCoord();
	}

	/**
	 * places the object on the map. You can extend this to get the coordinate.
 RenderCell may be placed without this method call. A regular renderblock
 is not spawned expect explicitely called.
	 *
	 * @param rS
	 * @param coord the position on the map
	 * @return itself
	 * @see #setPosition(com.bombinggames.wurfelengine.core.map.Position)
	 */
	public RenderCell spawn(RenderStorage rS, Coordinate coord) {
		setPosition(coord);
		Controller.getMap().setBlock(coord, getId(), getValue());
		return this;
	}
    
    @Override
	public void render(final GameView view) {
		if (!isHidden()) {
			if (hasSides()) {
				byte clipping = getClipping();
				if ((clipping & (1 << 1)) == 0) {
					renderSide(view, Side.TOP, staticShade);
				}
				if ((clipping & 1) == 0) {
					renderSide(view, Side.LEFT, staticShade);
				}
				if ((clipping & (1 << 2)) == 0) {
					renderSide(view, Side.RIGHT, staticShade);
				}
			} else {
				super.render(view);
			}
		}
	}
    
 /**
	 * Render the whole block at a custom position. Checks if hidden.
	 *
	 * @param view the view using this render method
	 * @param xPos rendering position projection space of center
	 * @param yPos rendering position projection space of center
	 */
	@Override
	public void render(final GameView view, final int xPos, final int yPos) {
		render(view, xPos, yPos, true);
	}

    /**
     * Renders the whole block at a custom position.
     * @param view the view using this render method
	 * @param xPos projection space of center
	 * @param yPos projection space of center
     * @param staticShade makes one side brighter, opposite side darker
     */
	public void render(final GameView view, final int xPos, final int yPos, final boolean staticShade) {
		if (!isHidden()) {
			if (hasSides()) {
				float scaling = getScaling();
				Color color;
				if (staticShade) {
					color = new Color(0.75f, 0.75f, 0.75f, 1);
				} else {
					color = new Color(0.5f, 0.5f, 0.5f, 1);
				}
				renderSide(
					view,
					(int) (xPos - VIEW_WIDTH2*scaling),
					(int) (yPos + (VIEW_HEIGHT-VIEW_DEPTH2)*scaling),
					Side.TOP,
					color
				);

				if (staticShade) {
					color = color.add(0.25f, 0.25f, 0.25f, 0);
				}
				renderSide(
					view,
					(int) (xPos-VIEW_WIDTH2*scaling),
					(int) (yPos-VIEW_DEPTH2*scaling),
					Side.LEFT,
					color
				);

				if (staticShade) {
					color = color.sub(0.25f, 0.25f, 0.25f, 0);
				}
				renderSide(
					view,
					xPos,
					(int) (yPos-VIEW_DEPTH2*scaling),
					Side.RIGHT,
					color
				);
			} else {
				super.render(view, xPos, yPos);
			}
		}
	}
       
	/**
     * Render a side of a block at the position of the internal coordinates.
	 * @param view
     * @param side The number identifying the side. 0=left, 1=top, 2=right
	 * @param staticShade
     */
    public void renderSide(
		final GameView view,
		final Side side,
		final boolean staticShade
	){

		//if vertex shaded then use different shading for each side
		Color color = tmpColor.set(Color.GRAY);
		if (Controller.getLightEngine() != null && !Controller.getLightEngine().isShadingPixelBased()) {
			color = Controller.getLightEngine().getColor(side, getPosition()).mul(color.r + 0.5f, color.g + 0.5f, color.b + 0.5f, color.a + 0.5f);
		}
		
		Point tmpPoint = getPoint();
        renderSide(
			view,
			tmpPoint,
            side,
            staticShade ?
				side == Side.RIGHT
				? color.sub(0.25f, 0.25f, 0.25f, 0)
				: (
					side == Side.LEFT
						? color.add(0.25f, 0.25f, 0.25f, 0)
						: color
					)
				: color//pass color if not shading static
        );
		
		//should be copied to this rendercell and updated only on change.
		byte health = getHealth();
		if (health < 100) {
			int damageOverlayStep = 0;
			if (health <= 50) {
				damageOverlayStep = 1;
			}
			if (health <= 25) {
				damageOverlayStep = 2;
			}
			
			if (damageOverlayStep > -1) {
				//render damage
				switch (side) {
					case LEFT:
						renderDamageOverlay(
							view,
							tmpPoint.add(-RenderCell.GAME_DIAGLENGTH2 / 2, 0, 0),
							(byte) (3 * damageOverlayStep)
						);
						break;
					case TOP:
						renderDamageOverlay(
							view,
							tmpPoint.add(0, 0, RenderCell.GAME_EDGELENGTH),
							(byte) (3 * damageOverlayStep + 1)
						);
						break;
					case RIGHT:
						renderDamageOverlay(
							view,
							tmpPoint.add(RenderCell.GAME_DIAGLENGTH2 / 2, 0, 0),
							(byte) (3 * damageOverlayStep + 2)
						);
						break;
					default:
						break;
				}
			}
		}
    }

	/**
	 * uses heap, projection space
	 * @param view the view using this render method
	 * @param xPos projection position
	 * @param yPos projection position
	 * @param side The number identifying the side. 0=left, 1=top, 2=right
	 * @param color when set overwrites value from light engine
	 */
	public void renderSide(final GameView view, final int xPos, int yPos, final Side side, Color color) {
		if (color == null){
			if (Controller.getLightEngine() != null && !Controller.getLightEngine().isShadingPixelBased()) {
				color = Controller.getLightEngine().getColor(side, getPosition());
			} else {
				color = Color.GRAY.cpy();
			}
		}

		byte id = getSpriteId();
		if (id <= 0) {
			return;
		}
		byte value = getSpriteValue();
		if (value < 0) {
			return;
		}

		Sprite sprite = new Sprite(getBlockSprite(id, value, side));
		//sprite.setRegion(getBlockSprite(id, value, side).getTexture());
		sprite.setPosition(xPos, yPos);
		if (getScaling() != 1) {
			sprite.setOrigin(0, 0);
			sprite.setScale(getScaling());
		}

		//if (color != null) {
		//	color.r *= getLightlevelR(side);
		//	if (color.r > 1) {//values above 1 can not be casted later
		//		color.r = 1;
		//	}
		//	color.g *= getLightlevelG(side);
		//	if (color.g > 1) {//values above 1 can not be casted later
		//		color.g = 1;
		//	}
		//	color.b *= getLightlevelB(side);
		//	if (color.b > 1) {//values above 1 can not be casted later
		//		color.b = 1;
		//	}
		sprite.setColor(color);
		sprite.draw(view.getProjectionSpaceSpriteBatch());
	}
	
	/**
	 * Draws a side of a cell at a custom position. Applies color before
	 * rendering and takes the lightlevel into account.
	 *
	 * @param view the view using this render method
	 * @param pos world space position
	 * @param side The number identifying the side. 0=left, 1=top, 2=right
	 * @param color a tint in which the sprite gets rendered. If null color gets
	 * ignored
	 */
	public void renderSide(final GameView view, Point pos, final Side side, Color color) {
		byte id = getSpriteId();
		if (id <= 0) {
			return;
		}
		byte value = getSpriteValue();
		if (value < 0) {
			return;
		}

		//lazy init
		GameSpaceSprite sprite;
		switch (side) {
			case LEFT:
				if (this.sprite == null) {
					this.sprite = new GameSpaceSprite(getBlockSprite(id, value, side), side, (byte) (aoFlags&255));
				}
				sprite = this.sprite;
				break;
			case TOP:
				if (side2 == null) {
					side2 = new GameSpaceSprite(getBlockSprite(id, value, side), side, (byte) ((aoFlags >> 8) & 255));
				}
				sprite = side2;
				break;
			default:
				if (side3 == null) {
					side3 = new GameSpaceSprite(getBlockSprite(id, value, side), side, (byte) ((aoFlags >> 16) & 255));
				}
				sprite = side3;
				break;
		}
		//sprite.setRegion(getBlockSprite(id, value, side).getTexture());
		sprite.setPosition(pos.getX(), pos.getY(), pos.getZ());
		if (getScaling() != 1) {
			sprite.setOrigin(0, 0);
			sprite.setScale(getScaling());
		}

		//if (color != null) {
		//	color.r *= getLightlevelR(side);
		//	if (color.r > 1) {//values above 1 can not be casted later
		//		color.r = 1;
		//	}
		//	color.g *= getLightlevelG(side);
		//	if (color.g > 1) {//values above 1 can not be casted later
		//		color.g = 1;
		//	}
		//	color.b *= getLightlevelB(side);
		//	if (color.b > 1) {//values above 1 can not be casted later
		//		color.b = 1;
		//	}
		int[] vertexcolor = this.color;
		byte sidecode = (byte) (side.getCode()*4);//get offset
		sprite.setColor(
			((vertexcolor[sidecode+0] >> (20 - 10 * Channel.Red.id)) & 0x3FF) / 1023f,
			((vertexcolor[sidecode+0] >> (20 - 10 * Channel.Green.id)) & 0x3FF) / 1023f,
			((vertexcolor[sidecode+0] >> (20 - 10 * Channel.Blue.id)) & 0x3FF) / 1023f,
			((vertexcolor[sidecode+1] >> (20 - 10 * Channel.Red.id)) & 0x3FF) / 1023f,
			((vertexcolor[sidecode+1] >> (20 - 10 * Channel.Green.id)) & 0x3FF) / 1023f,
			((vertexcolor[sidecode+1] >> (20 - 10 * Channel.Blue.id)) & 0x3FF) / 1023f,
			((vertexcolor[sidecode+2] >> (20 - 10 * Channel.Red.id)) & 0x3FF) / 1023f,
			((vertexcolor[sidecode+2] >> (20 - 10 * Channel.Green.id)) & 0x3FF) / 1023f,
			((vertexcolor[sidecode+2] >> (20 - 10 * Channel.Blue.id)) & 0x3FF) / 1023f,
			((vertexcolor[sidecode+3] >> (20 - 10 * Channel.Red.id)) & 0x3FF) / 1023f,
			((vertexcolor[sidecode+3] >> (20 - 10 * Channel.Green.id)) & 0x3FF) / 1023f,
			((vertexcolor[sidecode+3] >> (20 - 10 * Channel.Blue.id)) & 0x3FF) / 1023f
		);
		sprite.draw(view.getGameSpaceSpriteBatch());
    }

	/**
	 * helper function
	 *
	 * @param view
	 * @param camera
	 * @param pos
	 * @param value damage sprite value
	 */
	private void renderDamageOverlay(final GameView view, final Position pos, final byte value) {
		destruct.setSpriteValue(value);
		destruct.setPosition(pos);
		destruct.getColor().set(0.5f, 0.5f, 0.5f, 0.7f);
		destruct.render(view);
	}
	/**
	 * Update the block. Should only be used for cosmetic logic because this is only called for blocks which are covered by a camera.
	 * @param dt time in ms since last update
	 */
    public void update(float dt) {
    }

	@Override
	public void updateSpriteCache() {
		setValue(coord.getBlockValue());
		if (!hasSides()) {
			super.updateSpriteCache();
		}
	}

    @Override
    public char getSpriteCategory() {
        return 'b';
    }

	/**
	 * keeps reference
	 * @param coord 
	 */
	public void setPosition(Coordinate coord){
		this.coord = coord;
	}	
	
	/**
	 * Can light travel through object?
	 * @return
	 */
	public boolean isTransparent() {
		if (id==0) return true;
		return RenderCell.isTransparent(getSpriteId(),getSpriteValue());//sprite id because view related
	}
	
	/**
	 *
	 * @return
	 */
	public boolean isIndestructible() {
		return RenderCell.isIndestructible(id,value);//game logic related
	}
	
	/**
	 * Is the block a true block with three sides or does it get rendered by a
	 * single sprite?<br>
	 * This field is only used for representation (view) related data.<br>
	 * Only used for blocks. Entities should return <i>false</i>.
	 *
	 * @return <i>true</i> if it has sides, <i>false</i> if is rendered as a
	 * single sprite
	 */
	public boolean hasSides() {
		if (id == 0) {
			return false;
		}
		return RenderCell.hasSides(getSpriteId(),getSpriteValue());
	}

	/**
	 *
	 * @return
	 */
	public boolean isLiquid() {
		if (id == 0) {
			return false;
		}
		return RenderCell.isLiquid(id,value);
	}

	@Override
	public float getLightlevelR() {
		return (getLightlevel(Side.LEFT, (byte) 0, Channel.Red)
			+ getLightlevel(Side.TOP, (byte) 0, Channel.Red)
			+ getLightlevel(Side.RIGHT, (byte) 0, Channel.Red)) / 3f;
	}

	@Override
	public float getLightlevelG() {
		return (getLightlevel(Side.LEFT, (byte) 0, Channel.Green)
			+ getLightlevel(Side.TOP, (byte) 0, Channel.Green)
			+ getLightlevel(Side.RIGHT, (byte) 0, Channel.Green)) / 3f;
	}

	@Override
	public float getLightlevelB() {
		return (getLightlevel(Side.LEFT, (byte) 0, Channel.Blue)
			+ getLightlevel(Side.TOP, (byte) 0, Channel.Blue)
			+ getLightlevel(Side.RIGHT, (byte) 0, Channel.Blue)) / 3f;
	}

	/**
	 *
	 * @param side
	 * @param vertex 0-3
	 * @param channel
	 * @return range 0-2.
	 */
	public float getLightlevel(Side side, byte vertex, Channel channel) {
		byte colorBitShift = (byte) (20 - 10 * channel.id);
		return ((color[side.getCode()*4+vertex] >> colorBitShift) & 0x3FF) / 511f;
	}

	/**
	 * Stores the lightlevel factor for the whole cell and thereby overriding values for each side.
	 *
	 * @param lightlevel range 0-2 where 1 is default
	 */
	@Override
	public void setLightlevel(float lightlevel) {
		int color;
		if (lightlevel <= 0) {
			color = 0;
		} else {
			int l = (int) (lightlevel * 512);
			//clamp
			if (l > 1023) {
				l = 1023;
			}
			color = (l << 20) + (l << 10) + l;
		}
		for (int i = 0; i < this.color.length; i++) {
			this.color[i] = color;//512 base 10 for each color channel
		}
	}
	
	/**
	 * sets the light to 1
	 */
	public void resetLight(){
		for (int i = 0; i < color.length; i++) {
			color[i] = 537395712;//512 base 10 for each color channel
		}
	}

	/**
	 *
	 * @param lightlevel a factor in range [0-2]
	 * @param side
	 */
	public void setLightlevel(float lightlevel, Side side) {
		if (lightlevel < 0) {
			lightlevel = 0;
		}
		int l = (int) (lightlevel * 512);
		if (l > 1023) {
			l = 1023;
		}

		color[side.getCode()*4+0] = (l << 20) + (l << 10) + l;//RGB
		color[side.getCode()*4+1] = (l << 20) + (l << 10) + l;//RGB
		color[side.getCode()*4+2] = (l << 20) + (l << 10) + l;//RGB
		color[side.getCode()*4+3] = (l << 20) + (l << 10) + l;//RGB
	}
	
	/**
	 *
	 * @param lightlevel a factor in range [0-2]
	 * @param side
	 * @param vertex id [0-3]
	 */
	public void setLightlevel(float lightlevel, Side side, byte vertex) {
		if (lightlevel < 0) {
			lightlevel = 0;
		}
		int l = (int) (lightlevel * 512);
		if (l > 1023) {
			l = 1023;
		}

		color[side.getCode()*4+vertex] = (l << 20) + (l << 10) + l;//RGB
	}
	
		/**
	 *
	 * @param lightlevel a factor in range [0-2]
	 * @param side
	 * @param channel 0 = Red, 1 = Green, 2 = Blue
	 * @param vertex
	 */
	public void setLightlevel(float lightlevel, Side side, Channel channel, byte vertex) {
		if (lightlevel < 0) {
			lightlevel = 0;
		}
		
		byte colorBitShift = (byte) (20 - 10 * channel.id);
		
		int l = (int) (lightlevel * 512);
		if (l > 1023) {
			l = 1023;
		}
		
		color[side.getCode()*4+vertex] |= (l << colorBitShift);
	}
	
	/**
	 *
	 * @param lightlevel a factor in range [0-2]
	 * @param side
	 * @param channel 0 = Red, 1 = Green, 2 = Blue
	 * @param vertex
	 */
	public void addLightlevel(float lightlevel, Side side, Channel channel, byte vertex) {
		if (lightlevel < 0) {
			lightlevel = 0;
		}

		//amount of bits to be shifted
		byte colorBitShift = (byte) (20 - 10 * channel.id);

		int l = (int) (lightlevel * 512);
		if (l > 0x3FF) {
			l = 0x3FF;
		}

		int currentl = color[side.getCode()*4+vertex];
		//read value and add new
		int newl = ((currentl >> colorBitShift) & 0x3FF)+ l;
		//clamp at 10 bit
		if (newl > 0x3FF) {
			newl = 0x3FF;
		}
		
		//mask to write zeroes and then add new bits
		color[side.getCode()*4+vertex] = (currentl & ~(0x3FF <<colorBitShift))|(newl << colorBitShift);//write
	}
	
	/**
	 * 
	 * @return true if it hides the block behind and below
	 */
	public boolean hidingPastBlock() {
		return hasSides() && !isTransparent();
	}

	/**
	 * Set flags for the ambient occlusion algorithm to true
	 * @param side
	 */
	public void setAOFlagTrue(int side) {
		this.aoFlags |= 1 << side;//set n'th bit to true via OR operator
	}

	/**
	 * Set flags for the ambient occlusion algorithm to false
	 * @param side 
	 */
	public void setAOFlagFalse(int side) {
		this.aoFlags &= ~(1 << side);//set n'th bit to false via AND operator
	}

	/**
	 * byte 0: left side, byte 1: top side, byte 2: right side.<br>In each byte
	 * bit order: <br>
	 * 7 \ 0 / 1<br>
	 * -------<br>
	 * 6 | - | 2<br>
	 * -------<br>
	 * 5 / 4 \ 3<br>
	 *
	 * @return four bytes in an int
	 */
	public int getAOFlags() {
		return aoFlags;
	}

	/**
	 * Set all flags at once
	 *
	 * @param aoFlags
	 */
	public void setAoFlags(int aoFlags) {
		if (aoFlags!=this.aoFlags){
			if (sprite != null) {
				sprite.setAoFlags((byte) (aoFlags & 255));
			}
			if (side2 != null) {
				side2.setAoFlags((byte) ((aoFlags >> 8) & 255));
			}
			if (side3 != null) {
				side3.setAoFlags((byte) ((aoFlags >> 16) & 255));
			}
		}
			
		this.aoFlags = aoFlags;
	}

	/**
	 * a block is only clipped if every side is clipped
	 *
	 * @return
	 */
	public byte getClipping() {
		return clipping;
	}

	/**
	 * a block is only clipped if every side is clipped
	 *
	 * @return
	 */
	public boolean isFullyClipped() {
		return clipping == 0b111;
	}

	/**
	 *
	 */
	public void setClippedLeft() {
		clipping |= 1;
	}

	/**
	 *
	 */
	public void setClippedTop() {
		clipping |= 1 << 1;
	}

	/**
	 *
	 */
	public void setClippedRight() {
		clipping |= 1 << 2;
	}

	/**
	 * Makes every side visible
	 */
	public void setUnclipped() {
		clipping = 0;
	}

	/**
	 * adds the entity into a cell for depth sorting
	 *
	 * @param ent
	 */
	public void addCoveredEnts(AbstractEntity ent) {
		coveredEnts.add(ent);
	}

	@Override
	public boolean shouldBeRendered(Camera camera) {
		return id != 0
				&& !isFullyClipped()
				&& !isHidden()
				&& camera.inViewFrustum(coord);
	}

	@Override
	public LinkedList<RenderCell> getCoveredBlocks(RenderStorage rs) {
		if (lastRebuild < rebuildCoverList) {//only rebuild a maximum of one time per frame
			rebuildCovered(rs);
		}
		return covered;
	}
	
	public LinkedList<AbstractEntity> getCoveredEnts() {
		if (!coveredEnts.isEmpty()) {
			coveredEnts.sort((AbstractGameObject o1, AbstractGameObject o2) -> {
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
		}
		return coveredEnts;
	}

	/**
	 * Rebuilds the list of covered cells by this cell.
	 * @param rs 
	 */
	private void rebuildCovered(RenderStorage rs) {
		LinkedList<RenderCell> covered = this.covered;
		covered.clear();
		Coordinate nghb = getPosition();
		RenderCell cell;
		if (nghb.getZ() > 0) {
			cell = rs.getCell(nghb.add(0, 0, -1));//go down
			if (cell != null) {
				covered.add(cell);
			}
			//back right
			cell = rs.getCell(nghb.goToNeighbour(1));
			if (cell != null) {
				covered.add(cell);
			}
			//back left
			cell = rs.getCell(nghb.goToNeighbour(6));
			if (cell != null) {
				covered.add(cell);
			}
			
			//bottom front
			cell = rs.getCell(nghb.goToNeighbour(3).goToNeighbour(4));
//			if (cell != null) {
//				covered.add(cell);
//			}
			
			nghb.goToNeighbour(0).add(0, 0, 1);//go back to origin
		}
		
		cell = rs.getCell(nghb.goToNeighbour(1));//back right
		if (cell != null) {
			covered.add(cell);
		}

		cell = rs.getCell(nghb.goToNeighbour(6));//back left
		if (cell != null) {
			covered.add(cell);
		}
	

		nghb.goToNeighbour(3);//return to origin
		
		lastRebuild = Gdx.graphics.getFrameId();
	}

	/**
	 *
	 */
	public void clearCoveredEnts() {
		coveredEnts.clear();
	}

	/**
	 * get the health byte from the map.
	 *
	 * @return if no coordiante returns 100
	 */
	public byte getHealth() {
		if (coord == null) {
			return 100;
		}
		return Controller.getMap().getHealth(coord);
	}

	@Override
	public String toString() {
		return Integer.toHexString(hashCode()) + " @" + getPosition().toString() + " id: " + id + " value: " + value;
	}

	@Override
	public byte getSpriteId() {
		return id;
	}

	@Override
	public byte getSpriteValue() {
		return value;
	}

	/**
	 * It is advised to only change this value if the data stored in the map
	 * also changes.
	 *
	 * @param value game data value.
	 */
	public void setValue(byte value) {
		if (this.value != value) {
			//reset sides
			sprite = null;
			side2 = null;
			side3 = null;
		}
			
		this.value = value;
	}

	public static enum Channel {

		Red((byte) 0),
		Green((byte) 1),
		Blue((byte) 2);

		final byte id;

		Channel(byte id) {
			this.id = id;
		}
	}

}