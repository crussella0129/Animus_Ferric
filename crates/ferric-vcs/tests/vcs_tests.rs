use std::fs;
use std::process::Command;
use ferric_vcs::Vcs;
use tempfile::tempdir;

#[tokio::test]
async fn test_snapshot_and_revert() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    
    // Initialize git repo in the temp directory
    Command::new("git")
        .current_dir(root)
        .args(&["init"])
        .output()
        .unwrap();

    // Set config so commits don't fail in CI environments
    Command::new("git")
        .current_dir(root)
        .args(&["config", "user.name", "Test User"])
        .output()
        .unwrap();
    Command::new("git")
        .current_dir(root)
        .args(&["config", "user.email", "test@example.com"])
        .output()
        .unwrap();

    let vcs = Vcs::new(root);

    // Initial state
    let test_file = root.join("test.txt");
    fs::write(&test_file, "initial").unwrap();
    
    // Take snapshot at turn 1
    vcs.snapshot("test-session", 1).await.expect("Failed to snapshot");

    // Modify state
    fs::write(&test_file, "modified").unwrap();
    let other_file = root.join("other.txt");
    fs::write(&other_file, "hello").unwrap();
    
    // Take snapshot at turn 2
    vcs.snapshot("test-session", 2).await.expect("Failed to snapshot");
    
    // Revert to turn 1
    vcs.revert("test-session", 1).await.expect("Failed to revert");

    // Check state is back to turn 1
    assert_eq!(fs::read_to_string(&test_file).unwrap(), "initial");
    assert!(!other_file.exists());

    // Revert to turn 2
    vcs.revert("test-session", 2).await.expect("Failed to revert to turn 2");

    // Check state is back to turn 2
    assert_eq!(fs::read_to_string(&test_file).unwrap(), "modified");
    assert_eq!(fs::read_to_string(&other_file).unwrap(), "hello");
}
