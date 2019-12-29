package com.bombinggames.wurfelengine.core;

import java.io.File;
import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertTrue;
import org.junit.Test;

/**
 *
 * @author Benedikt S. Vogler
 */
public class WorkingDirectoryTest {
    
    public WorkingDirectoryTest() {
    }

    @org.junit.BeforeClass
    public static void setUpClass() throws Exception {
        WorkingDirectory.setApplicationName("test");
    }

    @org.junit.AfterClass
    public static void tearDownClass() throws Exception {
    }

    @org.junit.Before
    public void setUp() throws Exception {
    }

    @org.junit.After
    public void tearDown() throws Exception {
    }
    
	/**
	 * Test of setApplicationName method, of class WorkingDirectory.
	 */
	@Test
	public void testSetApplicationName() {
		System.out.println("setApplicationName");
		String applicationName = "test";
		WorkingDirectory.setApplicationName(applicationName);
		// TODO review the generated test code and remove the default call to fail.
		//fail("The test case is a prototype.");
	}

	/**
	 * Test of getWorkingDirectory method, of class WorkingDirectory.
	 */
	@Test
	public void testGetWorkingDirectory() {
		System.out.println("getWorkingDirectory");
		File expResult = null;
		File result = WorkingDirectory.getWorkingDirectory();
		assertTrue(result.isDirectory());
		//fail("The test case is a prototype.");
	}

	/**
	 * Test of getPlatform method, of class WorkingDirectory.
	 */
	@Test
	public void testGetPlatform() {
		System.out.println("getPlatform");
		WorkingDirectory.OS expResult = null;
		WorkingDirectory.OS result = WorkingDirectory.getPlatform();
		assertTrue(!result.equals(WorkingDirectory.OS.UNKNOWN));
	}

	/**
	 * Test of getMapsFolder method, of class WorkingDirectory.
	 */
	@Test
	public void testGetMapsFolder() {
		System.out.println("getMapsFolder");
		String expResult = "maps";
		File result = WorkingDirectory.getMapsFolder();
		assertEquals(expResult, result.getName());
	}

	/**
	 * Test of unpackMap method, of class WorkingDirectory.
	 */
	@Test
	public void testUnpackMap() {
		System.out.println("unpackMap not tested");
	}

	/**
	 * Test of delete method, of class WorkingDirectory.
	 */
	@Test
	public void testDelete() {
		System.out.println("delete");
		File wd = WorkingDirectory.getWorkingDirectory();
		assert(wd.exists());
		assertTrue(WorkingDirectory.delete());
		assert(!wd.exists());
	}

	/**
	 * Test of deleteDirectory method, of class WorkingDirectory.
	 */
	@Test
	public void testDeleteDirectory() {
		System.out.println("deleteDirectory not tested");
	}
    
}
