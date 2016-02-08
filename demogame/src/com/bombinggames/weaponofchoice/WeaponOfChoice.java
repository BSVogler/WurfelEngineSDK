package com.bombinggames.weaponofchoice;

import com.bombinggames.weaponofchoice.mainmenu.MainMenuScreen;
import com.bombinggames.wurfelengine.WE;
import com.bombinggames.wurfelengine.core.WorkingDirectory;
import com.bombinggames.wurfelengine.core.gameobjects.AbstractGameObject;

/**
 *Main class for game Weapon of Choice. This game was an entry for Ludum Dare #??.
 * @author Benedikt Vogler
 */
public class WeaponOfChoice {

    /**
     * @param args the command line arguments
     */
    public static void main(String[] args) {
        WE.setMainMenu(new MainMenuScreen());
		WorkingDirectory.setApplicationName("WeaponOfChoice");
		AbstractGameObject.setCustomSpritesheet("com/bombinggames/weaponofchoice/sprites/Spritesheet");
        WE.launch("Weapon of Choice - Made with WE V" + WE.VERSION, args);  
    }
    
}
