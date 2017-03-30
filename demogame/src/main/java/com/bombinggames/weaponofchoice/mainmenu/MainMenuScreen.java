package com.bombinggames.weaponofchoice.mainmenu;

import com.bombinggames.wurfelengine.core.AbstractMainMenu;
 
/**
 * The game state of the Main Menu.
 * @author Benedikt
 */
public class MainMenuScreen extends AbstractMainMenu {
    private static boolean loadMap = false;
 
    private static View View;
    private static Controller Controller;
    
    /**
     * Creates the main Menu
     */
    @Override
    public void init() {
        Controller = new Controller(); 
        View = new View();
    }

    
    @Override
    public void renderImpl(float delta) {
        Controller.update((int) (delta*1000));
        View.render(Controller);
        View.update(delta*1000);
    }

    @Override
    public void resize(int width, int height) {
    }

    @Override
    public void show() {
    }

    @Override
    public void hide() {
    }

    @Override
    public void pause() {
    }

    @Override
    public void resume() {
    }

    @Override
    public void dispose() {
    }
  
    /**
     * 
     * @return
     */
    public static Controller getController() {
        return Controller;
    }

    /**
     * 
     * @return
     */
    public static View getView() {
        return View;
    }

    /**
     * 
     * @return
     */
    public static boolean shouldLoadMap() {
        return loadMap;
    }

    /**
     * 
     * @param loadmap
     */
    public static void setLoadMap(boolean loadmap) {
        MainMenuScreen.loadMap = loadmap;
    }
}