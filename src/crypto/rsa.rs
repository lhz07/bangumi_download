use num_bigint::BigUint;
use num_traits::FromPrimitive;
use once_cell::sync::Lazy;
use rand::Rng;
use std::io::Write;
static N: Lazy<BigUint> = Lazy::new(|| {
    BigUint::parse_bytes(b"8686980c0f5a24c4b9d43020cd2c22703ff3f450756529058b1cf88f09b8602136477198a6e2683149659bd122c33592fdb5ad47944ad1ea4d36c6b172aad6338c3bb6ac6227502d010993ac967d1aef00f0c8e038de2e4d3bc2ec368af2e9f10a6f1eda4f7262f136420c07c331b871bf139f74f3010e3c4fe57df3afb71683", 16).unwrap()
});
static E: Lazy<BigUint> = Lazy::new(|| BigUint::from_u64(0x10001).unwrap());
static KEY_LENGTH: Lazy<usize> = Lazy::new(|| ((N.bits() + 7) / 8) as usize);

/// RSA 加密整个数据
pub fn rsa_encrypt(input: &[u8]) -> Vec<u8> {
    let mut output = Vec::new();
    let mut pos = 0;

    while pos < input.len() {
        let slice_size = (*KEY_LENGTH - 11).min(input.len() - pos);
        let slice = &input[pos..pos + slice_size];
        rsa_encrypt_slice(slice, &mut output);
        pos += slice_size;
    }

    output
}

/// 加密单个数据块
fn rsa_encrypt_slice(input: &[u8], w: &mut impl Write) {
    // 计算填充大小并生成随机填充
    let pad_size = *KEY_LENGTH - input.len() - 3;
    let mut pad_data = vec![0u8; pad_size];
    rand::rng().fill(&mut pad_data[..]);

    // 构建加密消息 [0x00, 0x02, pad..., 0x00, data]
    let mut msg = Vec::with_capacity(*KEY_LENGTH);
    msg.push(0x00);
    msg.push(0x02);

    for b in &pad_data {
        msg.push(b % 0xff + 1); // 确保非零填充
    }

    msg.push(0x00);
    msg.extend_from_slice(input);

    // 转换为大整数并进行 RSA 加密
    let msg_int = BigUint::from_bytes_be(&msg);
    let encrypted = msg_int.modpow(&E, &N);
    let encrypted_bytes = encrypted.to_bytes_be();

    // 处理前导零
    if encrypted_bytes.len() < *KEY_LENGTH {
        let zeros = vec![0u8; *KEY_LENGTH - encrypted_bytes.len()];
        w.write_all(&zeros).unwrap();
    }

    w.write_all(&encrypted_bytes).unwrap();
}

/// RSA 解密整个数据
pub fn rsa_decrypt(input: &[u8]) -> Vec<u8> {
    let mut output = Vec::new();
    let mut pos = 0;

    while pos < input.len() {
        let slice_size = (*KEY_LENGTH).min(input.len() - pos);
        let slice = &input[pos..pos + slice_size];
        rsa_decrypt_slice(slice, &mut output);
        pos += slice_size;
    }

    output
}

/// 解密单个数据块
fn rsa_decrypt_slice(input: &[u8], w: &mut impl Write) {
    // 转换为大整数并进行 RSA "解密" (使用相同指数)
    let msg_int = BigUint::from_bytes_be(input);
    let decrypted = msg_int.modpow(&E, &N);
    let decrypted_bytes = decrypted.to_bytes_be();

    // 查找填充结束位置 (0x00 之后的数据)
    let mut start = 0;
    for (i, &b) in decrypted_bytes.iter().enumerate().skip(1) {
        if b == 0 && i != 0 {
            start = i + 1;
            break;
        }
    }

    // 写入有效数据
    if start < decrypted_bytes.len() {
        w.write_all(&decrypted_bytes[start..]).unwrap();
    }
}
