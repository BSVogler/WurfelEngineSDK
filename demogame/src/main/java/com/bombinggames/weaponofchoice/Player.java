package com.bombinggames.weaponofchoice;

import com.bombinggames.wurfelengine.extension.UserControlledShooter;

/**
 *
 * @author Benedikt Vogler
 */
public class Player extends UserControlledShooter {

	private static final long serialVersionUID = 1L;

	public Player(int spritesPerDir, int height) {
		super(spritesPerDir, height);
		equipWeapon(new CustomWeapon((byte) 0, this));
		setMass(60f);
	}
}
