package com.bombinggames.weaponofchoice;

import com.badlogic.gdx.math.Vector2;
import com.bombinggames.wurfelengine.core.gameobjects.Controllable;
import com.bombinggames.wurfelengine.core.gameobjects.MovableEntity;
import com.bombinggames.wurfelengine.extension.PlayerWithWeapon;

/**
 *
 * @author Benedikt Vogler
 */
public class Player extends PlayerWithWeapon implements Controllable{

	private static final long serialVersionUID = 1L;
		
	public Player(int spritesPerDir, int height) {
		super(spritesPerDir, height);
		equipWeapon(new CustomWeapon((byte) 0, this));
		setMass(60f);
	}

	@Override
	public void walk(boolean up, boolean down, boolean left, boolean right, float walkingspeed, float dt) {
		
		if (up || down || left || right){

			//update the direction vector
			Vector2 dir = new Vector2(left ? -1 : (right ? 1 : 0f), up ? -1 : (down ? 1 : 0f));
			dir.nor().scl(walkingspeed);
			setHorMovement(dir);
		}
	}

	@Override
	public MovableEntity clone() throws CloneNotSupportedException {
		return super.clone();
	}

}
