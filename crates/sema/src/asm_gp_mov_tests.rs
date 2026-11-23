//! ASM3 P02H: check the generated standalone Mini-C MOV class source
//! without introducing compiler dependencies or new GNU asm admission.
use super::asm_gp_mov::reviewed_gp_memory_mov_source;

#[test]
fn reviewed_memory_mov_source_matrix_and_negative_clobbers() {
    for (bytes, suffix) in [(4u64, "l"), (8, "q")] {
        let template = format!("mov{suffix} %1,%0");
        for (output, input) in [("=r", "m"), ("=m", "r")] {
            for clobbers in [
                Vec::<String>::new(),
                vec!["memory".to_owned()],
                vec!["memory".to_owned(), "memory".to_owned()],
            ] {
                assert!(reviewed_gp_memory_mov_source(
                    &template, bytes, output, input, &clobbers,
                ));
            }
            for rejected in [
                vec!["cc".to_owned()],
                vec!["rax".to_owned()],
                vec!["memory".to_owned(), "cc".to_owned()],
            ] {
                assert!(!reviewed_gp_memory_mov_source(
                    &template, bytes, output, input, &rejected,
                ));
            }
            for malformed in [
                format!("lock {template}"),
                format!("mov{suffix} %0,%1"),
                format!("mov{suffix} %1, %0"),
            ] {
                assert!(!reviewed_gp_memory_mov_source(
                    &malformed, bytes, output, input, &[],
                ));
            }
        }
        for (output, input) in [
            ("=m", "m"), ("=r", "r"), ("+m", "r"), ("=&r", "m"), ("=r", "0"),
        ] {
            assert!(!reviewed_gp_memory_mov_source(
                &template, bytes, output, input, &[],
            ));
        }
        assert!(!reviewed_gp_memory_mov_source(
            &template, if bytes == 4 {8} else {4}, "=m", "r", &[],
        ));
    }
}
