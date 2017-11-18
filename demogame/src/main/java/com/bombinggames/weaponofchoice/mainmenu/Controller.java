package com.bombinggames.weaponofchoice.mainmenu;

import com.badlogic.gdx.Gdx;
import com.badlogic.gdx.Input;
import com.badlogic.gdx.InputProcessor;
import com.badlogic.gdx.assets.AssetManager;
import com.badlogic.gdx.audio.Music;
import com.badlogic.gdx.audio.Sound;
import com.badlogic.gdx.graphics.g2d.TextureAtlas;
import com.bombinggames.weaponofchoice.CustomGameController;
import com.bombinggames.weaponofchoice.CustomGameView;
import com.bombinggames.wurfelengine.WE;
import com.bombinggames.wurfelengine.core.loading.LoadingScreen;


/**
 * The controlelr of the main Menu manages the data.
 * @author Benedikt
 */
public class Controller {
    
    private final MenuItem[] menuItems = new MenuItem[2];
    private final Sound fx;
    
    /**
     * Creates a new Controller
     */
    public Controller() {
        TextureAtlas texture = new TextureAtlas(Gdx.files.internal("com/bombinggames/weaponofchoice/mainmenu/images/MainMenu.txt"), true);
                
        //menuItems[0] = new MenuItem(0, texture.getRegions().get(3));
        menuItems[0] = new MenuItem(1, texture.getRegions().get(1));
        //menuItems[2] = new MenuItem(2, texture.getRegions().get(0));
        menuItems[1] = new MenuItem(3, texture.getRegions().get(0));
        
        fx = Gdx.audio.newSound(Gdx.files.internal("com/bombinggames/weaponofchoice/mainmenu/click2.wav"));
        Gdx.input.setInputProcessor(new InputListener());
        
        //add asstes to queque
        AssetManager manager = WE.getAssetManager();
        manager.load("com/bombinggames/weaponofchoice/SpritesBig.txt", TextureAtlas.class);
        manager.load("com/bombinggames/weaponofchoice/sounds/melee.wav", Sound.class);
        manager.load("com/bombinggames/weaponofchoice/sounds/punch.wav", Sound.class);
        manager.load("com/bombinggames/weaponofchoice/sounds/reload.wav", Sound.class);
        manager.load("com/bombinggames/weaponofchoice/sounds/shot.wav", Sound.class);
        manager.load("com/bombinggames/weaponofchoice/sounds/shotgun.wav", Sound.class);
        manager.load("com/bombinggames/weaponofchoice/sounds/wiz.wav", Sound.class);
        manager.load("com/bombinggames/weaponofchoice/sounds/dudeldi.ogg", Sound.class);
        manager.load("com/bombinggames/weaponofchoice/sounds/bust.wav", Sound.class);
        manager.load("com/bombinggames/weaponofchoice/sounds/scream1.wav", Sound.class);
        manager.load("com/bombinggames/weaponofchoice/sounds/scream2.wav", Sound.class);
        manager.load("com/bombinggames/weaponofchoice/sounds/scream3.wav", Sound.class);
        manager.load("com/bombinggames/weaponofchoice/sounds/scream4.wav", Sound.class);
        manager.load("com/bombinggames/weaponofchoice/sounds/impactFlesh.wav", Sound.class);
        manager.load("com/bombinggames/weaponofchoice/sounds/fire.wav", Sound.class);
        manager.load("com/bombinggames/weaponofchoice/sounds/poop.wav", Sound.class);
        manager.load("com/bombinggames/weaponofchoice/sounds/thump.wav", Sound.class);
        manager.load("com/bombinggames/weaponofchoice/sounds/music.ogg", Music.class);
        manager.load("com/bombinggames/weaponofchoice/sounds/dead.ogg", Sound.class);
    }
    
    /**
     * updates game logic
     * @param delta
     */
    public void update(int delta){
        if (menuItems[0].isClicked()) { 
            MainMenuScreen.setLoadMap(false);
            fx.play();
            CustomGameController ctrl = new CustomGameController();
            WE.initAndStartGame(new LoadingScreen(), ctrl, new CustomGameView(ctrl));
        } else if (menuItems[1].isClicked()){
            fx.play();
            Gdx.app.exit();
        }
    }

    /**
     *
     * @return
     */
    public MenuItem[] getMenuItems() {
        return menuItems;
    }

    /**
     *
     */
    public void dispose(){
        fx.dispose();
    }

    private class InputListener implements InputProcessor {

        @Override
        public boolean keyDown(int keycode) {
            if (keycode == Input.Keys.ESCAPE)
                Gdx.app.exit();
            return true;
        }

        @Override
        public boolean keyUp(int keycode) {
            return true;
        }

        @Override
        public boolean keyTyped(char character) {
            return true;
        }

        @Override
        public boolean touchDown(int screenX, int screenY, int pointer, int button) {
            return true;
        }

        @Override
        public boolean touchUp(int screenX, int screenY, int pointer, int button) {
            return true;
        }

        @Override
        public boolean touchDragged(int screenX, int screenY, int pointer) {
            return true;
        }

        @Override
        public boolean mouseMoved(int screenX, int screenY) {
            return true;
        }

        @Override
        public boolean scrolled(int amount) {
            return true;
        }
    }
}