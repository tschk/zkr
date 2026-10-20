cat << 'PATCH' > fix_test.diff
--- src/personality.rs
+++ src/personality.rs
@@ -3338,6 +3338,7 @@
             .unwrap();

+        std::thread::sleep(std::time::Duration::from_millis(10));
         personality
             .record_event(&ConversationEvent {
                 epoch: personality.current_epoch(),
                 participant: "alice".into(),
PATCH
patch -p0 < fix_test.diff
