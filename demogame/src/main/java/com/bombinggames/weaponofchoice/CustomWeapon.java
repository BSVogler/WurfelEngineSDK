package com.bombinggames.weaponofchoice;

import com.badlogic.gdx.Gdx;
import com.badlogic.gdx.graphics.g2d.Sprite;
import com.bombinggames.wurfelengine.core.GameView;
import com.bombinggames.wurfelengine.core.gameobjects.AbstractGameObject;
import com.bombinggames.wurfelengine.core.gameobjects.MovableEntity;
import com.bombinggames.wurfelengine.extension.shooting.Weapon;

/**
 *
 * @author Benedikt Vogler
 */
public class CustomWeapon extends Weapon {
	private static final long serialVersionUID = 1L;
    
    public static void init(){
//        if (getSpritesheetBig() == null) {
//            setSpritesheetBig((TextureAtlas) WE.getAsset("com/bombinggames/WeaponOfChoice/SpritesBig.txt"));
//            for (TextureAtlas.AtlasRegion region : getSpritesheetBig().getRegions()) {
//                    region.flip(false, true);
//            }
//            for (Texture tex : getSpritesheetBig().getTextures()) {
//                tex.setFilter(Texture.TextureFilter.Nearest, Texture.TextureFilter.Nearest);
//            }
//        }
    }
    
	/**
	 * 
	 * @param id
	 * @param character 
	 */
    public CustomWeapon(byte id, MovableEntity character) {
        super(id, character);
		setHidden(true);
        
        switch (id){
            case 0:
                setFireSound("melee", false);
                setReload("wiz"); 
            break;
                
            case 1:
                setFireSound("shot", false);
                setReload("reload"); 
            break;
                
            case 2:
                setFireSound("punch", false);
                //setReload((Sound) WEMain.getInstance().manager.get("com/bombinggames/weaponofchoice/sounds/melee.wav")); 
            break;
                
            case 3:
                setFireSound("shotgun", false);
                //setReload((Sound) WE.getAsset("com/bombinggames/weaponofchoice/sounds/reload.wav")); 
            break;    

            case 4:
                setFireSound("bust", true);
                //setReload((Sound) WE.getAsset("com/bombinggames/weaponofchoice/sounds/reload.wav")); 
            break;
                                 
            case 5:
                setFireSound("poop", false);
                //setReload((Sound) WEMain.getInstance().manager.get("com/bombinggames/weaponofchoice/sounds/reload.wav")); 
            break;
                
            case 6:
                setFireSound("thump", false);
                //setReload((Sound) WE.getAsset("com/bombinggames/weaponofchoice/sounds/reload.wav")); 
            break;
                
            case 7:                
                setFireSound("fire", true);
                //setReload((Sound) WE.getAsset("com/bombinggames/weaponofchoice/sounds/reload.wav")); 
            break;     
        }
    }

	void renderHUD(GameView view, int x, int y) {
		Sprite sprite;
		sprite = new Sprite(AbstractGameObject.getSprite('i', (byte) 11, getSpriteValue())); // "canvas")
		sprite.flip(false, true);
		sprite.setX(Gdx.graphics.getWidth() / 2 - sprite.getWidth() / 2);
		sprite.setY(Gdx.graphics.getHeight() / 2 - 30);
		//sprite.scale(CustomWeapon.getScaling());
		sprite.draw(view.getProjectionSpaceSpriteBatch());
	}
}