package com.bombinggames.weaponofchoice;

import com.badlogic.gdx.ai.msg.MessageManager;
import com.badlogic.gdx.math.Vector3;
import com.bombinggames.wurfelengine.core.Events;
import com.bombinggames.wurfelengine.core.Gameobjects.MovableEntity;
import com.bombinggames.wurfelengine.core.Map.Point;

/**
 *An enemy which can follow a character.
 * @author Benedikt Vogler
 */
public class Enemy extends MovableEntity{

	private static final long serialVersionUID = 1L;
    private MovableEntity target;
    private int runningagainstwallCounter = 0;
    private Point lastPos;
    private static int killcounter = 0;
	private int mana = 0;
    
    public void init(){
       killcounter=0; 
    }
    
    /**
     * Zombie constructor. Use AbstractEntitiy.getInstance to create an zombie.
     * @param id
     */
    public Enemy(byte id) {
        super(id, 2);
        setObstacle(true);
        setDamageSounds(new String[]{"impactFlesh"});
    }

    @Override
    public void jump() {
        jump(5, true);
    }

    @Override
    public void update(float delta) {
        if (getPosition().toCoord().isInMemoryAreaHorizontal()) {
            //follow the target
            if (target != null) {
				Vector3 d = new Vector3();
                d.x = target.getPosition().getX()-getPosition().getX();
                d.y = target.getPosition().getY()-getPosition().getY();
				d.nor();
				d.z = getMovement().z;
				// update the movement vector
				setMovement(d.scl(0.4f));

                //attack
                if (
					target.getPosition().toCoord().equals(getPosition().toCoord())
				){
                    mana = ((int) (mana+delta));
                    if (mana >= 1000){
                        mana = 0;//reset
						//new EntityAnimation(new int[]{300}, true, false)
                        //new EntityAnimation(46, 0, new int[]{300}, true, false).spawn(getPosition().cpy());//spawn blood
						MessageManager.getInstance().dispatchMessage(
							this,
							target,
							Events.damage.getId(),
							(byte) 50
						);
                    }
                }
            }
            //update as usual
            super.update(delta);

            //if standing on same position as in last update
            if (getPosition().equals(lastPos))
                runningagainstwallCounter += delta;
            else {
                runningagainstwallCounter=0;
                lastPos = getPosition();
            }

            //jump after some time
            if (runningagainstwallCounter > 500) {
                jump();
               mana = 0;
                runningagainstwallCounter=0;
            }
        }
    }

    /**
     * Set the target which the zombie follows.
     * @param target an character
     */
    public void setTarget(MovableEntity target) {
        this.target = target;
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