/*
 * If this software is used for a game the official „Wurfel Engine“ logo or its name must be visible in an intro screen or main menu.
 * 
 * Copyright 2014 Benedikt Vogler.
 * All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions are met:
 *
 * * Redistributions of source code must retain the above copyright notice, 
 *   this list of conditions and the following disclaimer.
 * * Redistributions in binary form must reproduce the above copyright notice, 
 *   this list of conditions and the following disclaimer in the documentation 
 *   and/or other materials provided with the distribution.
 * * Neither the name of Benedikt Vogler nor the names of its contributors 
 *   may be used to endorse or promote products derived from this software without specific
 *   prior written permission.
 *
 * THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
 * AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE
 * LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
 * CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
 * SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
 * INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
 * CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
 * ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
 * POSSIBILITY OF SUCH DAMAGE.
 */
package com.bombinggames.wurfelengine.core;

import com.badlogic.gdx.Gdx;
import com.badlogic.gdx.graphics.Color;
import com.badlogic.gdx.graphics.GL20;
import com.badlogic.gdx.graphics.glutils.ShapeRenderer;
import com.bombinggames.wurfelengine.WE;
import com.bombinggames.wurfelengine.mapeditor.EditorView;
import java.text.DecimalFormat;
import java.text.NumberFormat;

/**
 * Shows data for developers. Also has some tools like buttons to edito the map.
 * @author Benedikt Vogler
 */
public class DevTools {

    /**
     *The visualised width of every data
     */
    public static final int WIDTH=3;
	public static final int maxStepsVisualized = 100;
	/**
	 * stores the time of a frame IN seconds.
	 */
    private float[] data = new float[100];
    private final int leftOffset, topOffset, maxHeight;
    private int field;//the current field number
    private boolean visible = true;
    private StringBuilder memoryText;
    private long freeMemory;
    private long allocatedMemory;
    private long maxMemory;
    private long usedMemory;

  /**
	 *
	 * @param xPos the position of the diagram from left
	 * @param yPos the position of the diagram from top
	 */
	public DevTools(final int xPos, final int yPos) {
		this.leftOffset = xPos;
		this.topOffset = yPos;
		maxHeight = 150;
	}
	
	/**
	 * the amount of frame times stored
	 * @param cap 
	 */
	public void setCapacity(int cap){
		data = new float[cap];
		field = 0;
	}
	
	/**
	 * sets the frame time of every frame to 0
	 */
	public void clear(){
		for (int i = 0; i < data.length; i++) {
			data[i] = 0;
		}
		field = 0;
	}
    
    /**
	 * Updates the diagramm
	 *
	 */
	public void update() {
		float rdt = Gdx.graphics.getRawDeltaTime();
		field++;//move to next field
		if (field >= data.length) {
			field = 0; //start over           
		}
		data[field] = rdt;//save delta time
        
        Runtime runtime = Runtime.getRuntime();
        NumberFormat format = NumberFormat.getInstance();

        maxMemory = runtime.maxMemory();
        allocatedMemory = runtime.totalMemory();
        freeMemory = runtime.freeMemory();
        usedMemory = allocatedMemory-freeMemory;

        memoryText = new StringBuilder(100);
        memoryText.append(format.format(usedMemory / 1024));
        memoryText.append("/").append(format.format(allocatedMemory / 1024)).append(" MB");
//        memoryText.append("free: ").append(format.format(freeMemory / 1024));
//        memoryText.append("allocated: ").append(format.format(allocatedMemory / 1024));
//        memoryText.append("max: ").append(format.format(maxMemory / 1024));
//        memoryText.append("total free: ").append(format.format((freeMemory + (maxMemory - allocatedMemory)) / 1024));
    }
    
    /**
     *Renders the diagramm. The batches should be closed before calling this method.
     * @param view if from class {@link EditorView} removes or shows the buttons
     */
    public void render(final GameView view){
        if (visible){
            //draw FPS-String
            view.drawString("FPS: "+ Gdx.graphics.getFramesPerSecond(), 15, 15,Color.WHITE.cpy(), true);
            view.drawString("Sprites: "+ view.getRenderedSprites(), 15, 30,Color.WHITE.cpy(), true);
            
            //draw diagramm
            ShapeRenderer shr = view.getShapeRenderer();
            Gdx.gl.glEnable(GL20.GL_BLEND);
            Gdx.gl.glBlendFunc(GL20.GL_SRC_ALPHA,GL20.GL_ONE_MINUS_SRC_ALPHA);
            Gdx.gl.glLineWidth(1);
            
            
            shr.begin(ShapeRenderer.ShapeType.Filled);
            //background
            shr.setColor(new Color(0.5f, 0.5f, 0.5f, 0.2f));
			int xPos = leftOffset;
			int yPos = (int) (Gdx.graphics.getHeight()/view.getEqualizationScale()-topOffset);
			
            shr.rect(xPos, yPos, getWidth(), -maxHeight);
            
            //render RAM
			shr.setColor(new Color(.2f, 1, .2f, 0.5f));
			shr.rect(
				xPos,
				yPos,
				usedMemory*getWidth() / allocatedMemory,
				-20
			);

			shr.setColor(new Color(0.5f, 0.5f, 0.5f, 0.6f));
			shr.rect(
				xPos + usedMemory * getWidth() / allocatedMemory,
				yPos,
				getWidth() - getWidth() * usedMemory / allocatedMemory,
				-20
			);
			
            //render current field bar
            shr.getColor().set(getSavedDelta(field)/0.0333f-0.5f, 0, 1, 0.8f);
            shr.rect(xPos+WIDTH*field%(WIDTH*maxStepsVisualized),
                yPos-maxHeight,
                WIDTH,
                data[field]*3000
            );
            
            shr.end();
            
            //render lines
            shr.begin(ShapeRenderer.ShapeType.Line);
            
            //render steps
            shr.setColor(Color.GRAY);
            shr.line(xPos, yPos-maxHeight, xPos+getWidth(), yPos-maxHeight);
            shr.line(xPos, yPos-maxHeight+0.0166f*3000, xPos+getWidth(), yPos-maxHeight+0.0166f*3000);
            shr.line(xPos, yPos-maxHeight+0.0333f*3000, xPos+getWidth(), yPos-maxHeight+0.0333f*3000);
            shr.line(xPos, yPos-maxHeight+0.0666f*3000, xPos+getWidth(), yPos-maxHeight+0.0666f*3000);
			//render each delta field in memory
			int from = Math.max((field /maxStepsVisualized-1)*maxStepsVisualized,0);
			int to = ((field /maxStepsVisualized+1)*maxStepsVisualized)%data.length-1;
            for (int i = from; i < to; i++) {
				shr.end();
				shr.getColor().set(getSavedDelta(i+1)/0.0333f-0.5f, 1f-getSavedDelta(i+1)/0.0333f, 0, 0.9f);
				shr.begin(ShapeRenderer.ShapeType.Line);
				if (getSavedDelta(i + 1)>0 && i < field){
					shr.line(
						xPos + WIDTH * (i + 0.5f) % (WIDTH * maxStepsVisualized),
						yPos + getSavedDelta(i) * 3000 - maxHeight,
						xPos + WIDTH * (i + 1.5f) % (WIDTH * maxStepsVisualized),
						yPos + getSavedDelta(i + 1) * 3000 - maxHeight
					);
				}

			}
            
            //render average values       
            float avg = getAverageDelta(WE.getCVars().getValueI("numFramesAverageDelta"));
           if (avg > 0) {
				//delta values
				shr.getColor().set(0, 0.3f, 0.8f, 0.7f);
				shr.line(
					xPos,
					yPos + avg* 3000 - maxHeight,
					xPos + getWidth(),
					yPos + avg* 3000 - maxHeight
				);
				String deltaT = new DecimalFormat("#.##").format(avg * 1000);
				view.drawString("d: " + deltaT, xPos, (int) (yPos - maxHeight + avg * 3000), new Color(0, 0.3f, 0.8f, 0.7f), true);
			}
           
            shr.end(); 
            
            view.drawString(memoryText.toString(), xPos, yPos,Color.WHITE.cpy(), true);
            Gdx.gl.glDisable(GL20.GL_BLEND);
        }
    }
    
	/**
	 * Get a recorded frame time value. The time between savings is at least the
	 * timeStepMin.
	 *
	 * @param pos the array position
	 * @return time in s
	 */
	public float getSavedDelta(int pos) {
		return data[pos];
	}
	
    /**
     * Returns the average delta time over every recorded frame.
     * @return time in s
     */
    public float getAverageDelta(){
        float avg = 0;
        int length = 0;
        for (float rdt : data) {
            avg += rdt;
            if (rdt > 0) length++;//count how many field are filled
        }
        if (length > 0) avg /= length;
        return avg;
    }
	
	/**
	 * Get the avererage raw delta time over the last n steps
	 * @param lastNSteps
	 * @return time in s
	 */
	public float getAverageDelta(int lastNSteps) {
		float avg = 0;
		int length = 0;
		for (int i = 0; i < lastNSteps; i++) {
			float rdt = data[(field - i + data.length) % data.length];
			avg += rdt;
			if (rdt > 0) {
				length++;//count how many field are filled
			}
		}
		if (length > 0) {
			avg /= length;
		}
		return avg;
	}

    /**
     * Is the diagramm visible?
     * @return 
     */
    public boolean isVisible() {
        return visible;
    }

   /**
    * Set the FPSdiag visible. You must nevertheless call render() to let it appear.
    * @param visible 
    */
    public void setVisible(final boolean visible) {
        this.visible = visible;
    }

    /**
     *
     * @return 
     */
    public int getLeftOffset() {
        return leftOffset;
    }

    /**
     * 
     * @return Y-Up
     */
    public int getTopOffset() {
        return topOffset;
    }

    /**
     * Width of FPS diag.
     * @return in pixels
     */
    public int getWidth() {
		int width = WIDTH*data.length;
		if (width> WIDTH*maxStepsVisualized)
			width = (WIDTH*maxStepsVisualized);
		return width;
    }

	/**
	 * CSV (<a href="https://tools.ietf.org/html/rfc4180.html">RFC 4180</a>) compliant.
	 * @return in ms
	 */
	public String getDataAsString() {
			StringBuilder content = new StringBuilder(data.length);
			char separator = ',';
			int c=0;
			for (float f : data) {
				if (f!=0) {
					c++;
					content.append(f*1000);
					content.append(separator);
				}
			}
			System.out.println("Got "+c+" frame times");
			return content.toString();
	}
}
