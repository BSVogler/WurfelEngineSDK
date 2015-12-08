package com.bombinggames.weaponofchoice;

import com.badlogic.gdx.math.Vector2;
import com.bombinggames.wurfelengine.core.Gameobjects.Controllable;
import com.bombinggames.wurfelengine.core.Gameobjects.MovableEntity;
import com.bombinggames.wurfelengine.core.Gameobjects.PlayerWithWeapon;

/**
 *
 * @author Benedikt Vogler
 */
public class Player extends PlayerWithWeapon implements Controllable{

	private static final long serialVersionUID = 1L;
		
	public Player(int spritesPerDir, int height) {
		super(spritesPerDir, height);
		equipWeapon(new CustomWeapon((byte) 0, this));
	}

	@Override
	public void walk(boolean up, boolean down, boolean left, boolean right, float walkingspeed, float dt) {
		
		if (up || down || left || right){

			//update the direction vector
			Vector2 dir = new Vector2();

			if (up)    dir.y += -1;
			if (down)  dir.y += 1;
			if (left)  dir.x += -1;
			if (right) dir.x += 1;
			dir.nor().scl(walkingspeed);
			setHorMovement(dir);
		}
	}

	@Override
	public MovableEntity clone() throws CloneNotSupportedException {
		return super.clone();
	}

}
