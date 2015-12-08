/*******************************************************************************
 * Copyright 2014 See AUTHORS file.
 * 
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 * 
 *   http://www.apache.org/licenses/LICENSE-2.0
 * 
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 ******************************************************************************/

package com.badlogic.gdx.ai.pfa.indexed;

import com.badlogic.gdx.ai.pfa.Connection;
import com.badlogic.gdx.utils.Array;

/** Interface for a node used by the {@link IndexedAStarPathFinder}.
 * 
 * @param <N> Type of node extending {@link IndexedNode}
 * 
 * @author davebaol */
public interface IndexedNode<N extends IndexedNode<N>> {

	/** Returns the index of this {@code IndexedNode}. The indexes of the nodes of an {@link IndexedGraph} must be a sequence
	 * starting from index 0. */
	public int getIndex ();

	/** Returns an array of {@link Connection connections} outgoing from this {@code IndexedNode}. */
	public Array<Connection<N>> getConnections ();
}
