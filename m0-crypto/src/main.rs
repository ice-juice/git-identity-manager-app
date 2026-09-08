// M0 加密栈验证 + Argon2id 参数标定
// 目标：找到在本机上耗时约 500ms 的 Argon2id 参数，并验证整套加密 crate 可编译可运行。

use std::time::Instant;

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::{
    aead::{Aead, KeyInit, OsRng, Payload},
    XChaCha20Poly1305, XNonce,
};
use hkdf::Hkdf;
use rand_core::RngCore;
use sha2::Sha256;
use zeroize::Zeroize;

fn derive_key(mem_kib: u32, iters: u32, par: u32, salt: &[u8], pwd: &[u8]) -> [u8; 32] {
    let params = Params::new(mem_kib, iters, par, Some(32)).expect("参数非法");
    let a2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut out = [0u8; 32];
    a2.hash_password_into(pwd, salt, &mut out).expect("派生失败");
    out
}

fn main() {
    println!("== M0 加密栈验证 ==");
    println!("cpu 逻辑核心数: {}", std::thread::available_parallelism().map(|n| n.get()).unwrap_or(0));

    // 1) Argon2id 参数标定：目标约 500ms
    println!("\n== Argon2id 参数标定（目标 ~500ms）==");
    let salt = b"0123456789abcdef"; // 16B，仅标定用
    let pwd = b"correct horse battery staple";
    // 候选参数：(内存 KiB, 迭代次数, 并行度)
    let candidates: &[(u32, u32, u32)] = &[
        (64 * 1024, 3, 4),   // 设计文档原默认
        (128 * 1024, 3, 4),
        (192 * 1024, 3, 4),
        (256 * 1024, 3, 4),
        (384 * 1024, 3, 4),
        (256 * 1024, 4, 4),
        (512 * 1024, 3, 4),
    ];
    let mut recommended: Option<(u32, u32, u32, u128)> = None;
    for &(mem, it, par) in candidates {
        // 预热一次，再取三次平均，减少抖动
        let _ = derive_key(mem, it, par, salt, pwd);
        let mut total = 0u128;
        let runs = 3;
        for _ in 0..runs {
            let t = Instant::now();
            let mut k = derive_key(mem, it, par, salt, pwd);
            total += t.elapsed().as_millis();
            k.zeroize();
        }
        let avg = total / runs as u128;
        println!("  mem={:>4}MiB it={} par={} -> {:>5} ms/次", mem / 1024, it, par, avg);
        // 选最接近但不小于 400ms 的档位作为推荐（<=600 优先）
        if avg >= 400 {
            let better = match recommended {
                None => true,
                Some((_, _, _, prev)) => {
                    // 更接近 500 的胜出
                    let d_new = (avg as i128 - 500).abs();
                    let d_old = (prev as i128 - 500).abs();
                    d_new < d_old
                }
            };
            if better {
                recommended = Some((mem, it, par, avg));
            }
        }
    }
    match recommended {
        Some((mem, it, par, avg)) => println!(
            "  >> 推荐参数: mem={}MiB, iters={}, parallelism={} (实测 {} ms)",
            mem / 1024,
            it,
            par,
            avg
        ),
        None => println!("  >> 所有候选均 <400ms，可适当上调内存/迭代"),
    }

    // 2) HKDF-SHA256 子密钥派生（模拟从主密钥派生用途子密钥）
    println!("\n== HKDF-SHA256 子密钥派生 ==");
    let mut mk = [0u8; 32];
    OsRng.fill_bytes(&mut mk);
    let hk = Hkdf::<Sha256>::new(Some(b"gam-salt"), &mk);
    let mut sub_file = [0u8; 32];
    let mut sub_name = [0u8; 32];
    hk.expand(b"key-file-encryption", &mut sub_file).unwrap();
    hk.expand(b"sync-object-name", &mut sub_name).unwrap();
    println!("  file 子密钥前4字节: {:02x?}", &sub_file[..4]);
    println!("  name 子密钥前4字节: {:02x?}", &sub_name[..4]);

    // 3) XChaCha20-Poly1305 加解密round trip（模拟私钥入库）
    println!("\n== XChaCha20-Poly1305 加解密验证 ==");
    let cipher = XChaCha20Poly1305::new((&sub_file).into());
    let mut nonce_bytes = [0u8; 24];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = XNonce::from_slice(&nonce_bytes);
    let plaintext = b"-----BEGIN OPENSSH PRIVATE KEY----- (fake) -----END-----";
    let aad = b"identity=techn4950"; // 附加认证数据
    let ct = cipher
        .encrypt(nonce, Payload { msg: plaintext, aad })
        .expect("加密失败");
    let pt = cipher
        .decrypt(nonce, Payload { msg: &ct, aad })
        .expect("解密失败");
    assert_eq!(pt, plaintext, "解密结果与原文不一致");
    println!("  明文 {} 字节 -> 密文 {} 字节 -> 解密还原一致: OK", plaintext.len(), ct.len());

    // 篡改检测：改一个字节应导致解密失败
    let mut tampered = ct.clone();
    tampered[0] ^= 0x01;
    let bad = cipher.decrypt(nonce, Payload { msg: &tampered, aad });
    println!("  篡改密文后解密应失败: {}", if bad.is_err() { "OK（已拒绝）" } else { "异常（未拒绝！）" });

    mk.zeroize();
    sub_file.zeroize();
    sub_name.zeroize();

    println!("\n== 全部加密原语在本机编译并运行通过 ==");
}
