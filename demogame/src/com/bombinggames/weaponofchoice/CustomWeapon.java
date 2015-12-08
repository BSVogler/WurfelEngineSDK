package com.bombinggames.weaponofchoice;

import com.bombinggames.wurfelengine.core.GameView;
import com.bombinggames.wurfelengine.core.Gameobjects.MovableEntity;
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
    
    public CustomWeapon(byte id, MovableEntity character) {
        super(id, character);
        
        switch (id){
            case 0:
                setFireSound("melee", true);
                setReload("wiz"); 
            break;
                
            case 1:
                setFireSound("shot", true);
                setReload("reload"); 
            break;
                
            case 2:
                setFireSound("punch", true);
                //setReload((Sound) WEMain.getInstance().manager.get("com/bombinggames/WeaponOfChoice/Sounds/melee.wav")); 
            break;
                
            case 3:
                setFireSound("shotgun", true);
                //setReload((Sound) WE.getAsset("com/bombinggames/WeaponOfChoice/Sounds/reload.wav")); 
            break;    

            case 4:
                setFireSound("bust", true);
                //setReload((Sound) WE.getAsset("com/bombinggames/WeaponOfChoice/Sounds/reload.wav")); 
            break;
                                 
            case 5:
                setFireSound("poop.wav", true);
                //setReload((Sound) WEMain.getInstance().manager.get("com/bombinggames/WeaponOfChoice/Sounds/reload.wav")); 
            break;
                
            case 6:
                setFireSound("thump", true);
                //setReload((Sound) WE.getAsset("com/bombinggames/WeaponOfChoice/Sounds/reload.wav")); 
            break;
                
            case 7:                
                setFireSound("fire", true);
                //setReload((Sound) WE.getAsset("com/bombinggames/WeaponOfChoice/Sounds/reload.wav")); 
            break;     
        }
    }

	void renderHUD(GameView view, int x, int y) {
		
	}
}