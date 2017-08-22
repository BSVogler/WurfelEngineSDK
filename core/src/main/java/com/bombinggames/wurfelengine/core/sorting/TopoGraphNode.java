package com.bombinggames.wurfelengine.core.sorting;

import com.badlogic.gdx.Gdx;
import com.bombinggames.wurfelengine.core.gameobjects.AbstractEntity;
import com.bombinggames.wurfelengine.core.gameobjects.AbstractGameObject;
import com.bombinggames.wurfelengine.core.map.Coordinate;
import com.bombinggames.wurfelengine.core.map.rendering.RenderCell;
import com.bombinggames.wurfelengine.core.map.rendering.RenderChunk;
import com.bombinggames.wurfelengine.core.map.rendering.RenderStorage;
import java.util.LinkedList;

/**
 * a graph node which references to a cell.
 * @author Benedikt S. Vogler
 */
public class TopoGraphNode {

	/**
	 * frame number of last rebuild
	 */
	private static long rebuildCoverList = 0;

	/**
	 * Only relevant to topological depth sort {@link TopologicalSort}. Sets a
	 * flag which causes the baking of the coverlist. This causes every field
	 * wich contains the covered neighbors to be rebuild. Used to prenvent
	 * duplicate graph rebuilds in one frame.
	 */
	public static void flagRebuildCoverList() {
		rebuildCoverList = Gdx.graphics.getFrameId();
	}

	/**
	 * bit position = camera id
	 */
	public static int currentMarkedFlag;

	/**
	 * inverses the dirty flag comparison so everything marked is now unmarked.
	 * used to mark the visited obejcts with depthsort.
	 *
	 * @param id
	 */
	public static void inverseMarkedFlag(int id) {
		currentMarkedFlag ^= 1 << id;
	}


	
	/**
	 * frame number to avoid multiple calculations in one frame
	 */
	private transient long lastRebuild;

	/**
	 * Stores references to neighbor blocks which are covered. For topological
	 * sort.
	 */
	private final LinkedList<TopoGraphNode> covered = new LinkedList<>();
	/**
	 * for topological sort. At the end contains both entities and blocks
	 */
	private final LinkedList<AbstractEntity> coveredEnts = new LinkedList<>();

	private RenderCell cell;

	/**
	 * flag used for depth sorting
	 */
	private int marked;

	public TopoGraphNode(RenderCell cell) {
		if (cell==null)
			throw new IllegalArgumentException("cell can not be null");
		this.cell = cell;
	}
	
	
	/**
	 * adds the entity into a cell for depth sorting
	 *
	 * @param ent
	 */
	public void addCoveredEnts(AbstractEntity ent) {
		coveredEnts.add(ent);
	}
	
	public LinkedList<AbstractEntity> getCoveredEnts() {
		if (!coveredEnts.isEmpty()) {
			coveredEnts.sort((AbstractGameObject o1, AbstractGameObject o2) -> {
				float d1 = o1.getDepth();
				float d2 = o2.getDepth();
				if (d1 > d2) {
					return 1;
				} else {
					if (d1 == d2) {
						return 0;
					}
					return -1;
				}
			});
		}
		return coveredEnts;
	}

	/**
	 * Rebuilds the list of covered cells by this cell.
	 * @param rs 
	 */
	private void rebuildCovered(RenderStorage rs) {
		LinkedList<TopoGraphNode> covered = this.covered;
		covered.clear();
		Coordinate nghb = cell.getCoord();
		RenderCell cell;
		if (nghb.getZ() > 0) {
			cell = rs.getCell(nghb.add(0, 0, -1));//go down
			if (cell != RenderChunk.CELLOUTSIDE) {
				covered.add(cell.getTopoNode());
			}
			//back right
			cell = rs.getCell(nghb.goToNeighbour(1));
			if (cell != RenderChunk.CELLOUTSIDE) {
				covered.add(cell.getTopoNode());
			}
			//back left
			cell = rs.getCell(nghb.goToNeighbour(6));
			if (cell != RenderChunk.CELLOUTSIDE) {
				covered.add(cell.getTopoNode());
			}
			
			//bottom front
			cell = rs.getCell(nghb.goToNeighbour(3).goToNeighbour(4));
//			if (cell != null) {
//				covered.add(cell);
//			}
			
			nghb.goToNeighbour(0).add(0, 0, 1);//go back to origin
		}
		
		cell = rs.getCell(nghb.goToNeighbour(1));//back right
		if (cell != RenderChunk.CELLOUTSIDE) {
			covered.add(cell.getTopoNode());
		}

		cell = rs.getCell(nghb.goToNeighbour(6));//back left
		if (cell != RenderChunk.CELLOUTSIDE) {
			covered.add(cell.getTopoNode());
		}
	

		nghb.goToNeighbour(3);//return to origin
		
		lastRebuild = Gdx.graphics.getFrameId();
	}

	/**
	 *
	 */
	public void clearCoveredEnts() {
		coveredEnts.clear();
	}

	public LinkedList<TopoGraphNode> getCoveredBlocks(RenderStorage rs) {
		if (lastRebuild < rebuildCoverList) {//only rebuild a maximum of one time per frame
			rebuildCovered(rs);
		}
		return covered;
	}

	/**
	 * Check if it is marked in this frame. Used for depth sorting.
	 *
	 * @param cameraId camera cameraId
	 * @return
	 * @see
	 * com.bombinggames.wurfelengine.core.sorting.TopologicalSort#visit(RenderCell)
	 */
	public final boolean isMarkedDS(final int cameraId) {
		return ((marked >> cameraId) & 1) == ((currentMarkedFlag >> cameraId) & 1);
	}

	/**
	 * Marks as visited in the depth sorting algorithm.
	 *
	 * @param id camera id
	 * @see
	 * com.bombinggames.wurfelengine.core.sorting.TopologicalSort#visit(RenderCell)
	 */
	public void markAsVisitedDS(final int id) {
		marked ^= (-((currentMarkedFlag >> id) & 1) ^ marked) & (1 << id);
	}

	/**
	 * get the represented cell instance
	 *
	 * @return
	 */
	public RenderCell getCell() {
		return cell;
	}

	/**
	 * 
	 * @param cell 
	 */
	public void setCell(RenderCell cell) {
		if (cell==null)
			throw new IllegalArgumentException("cell can not be null");
		this.cell = cell;
	}
	
	
	
	
}
