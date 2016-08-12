package com.bombinggames.weaponofchoice;

import com.bombinggames.wurfelengine.core.gameobjects.MovableEntity;

/**
 * An enemy which can follow a character.
 *
 * @author Benedikt Vogler
 */
public class Enemy extends MovableEntity {

	private static final long serialVersionUID = 1L;
	private static int killcounter = 0;

	public void init() {
		killcounter = 0;
	}

	/**
	 * Zombie constructor.
	 * @param target
	 */
	public Enemy(MovableEntity target) {
		super((byte) 44, 2);
		setObstacle(true);
		setDamageSounds(new String[]{"impactFlesh"});
		EnemyAI ai = new EnemyAI();
		ai.setTarget(target);
		addComponent(ai);
	}

	@Override
	public void jump() {
		jump(5, true);
	}

	@Override
	public void dispose() {
		killcounter++;
		super.dispose();
	}

	public static int getKillcounter() {
		return killcounter;
	}
}
