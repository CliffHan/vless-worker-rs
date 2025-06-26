# VLESS-WORKER-RS

在 Cloudflare Worker 上部署一个 VLESS Server，用 Rust 实现。

参考了 JS 版本的实现：https://github.com/AliAlmasi/vless-cf-worker

## 使用方法

1. 编译并发布到 Cloudflare。
2. 设置 UUID 环境变量（也可以修改代码中默认 UUID）。
3. 设置客户端 VLESS 配置，包括：  
    a. 地址为发布后的域名，端口为 443。  
    b. 用户 ID 为 UUID 环境变量，不设置的话默认为代码中的 UUID；流控为空；加密方式为 none。  
    c. 底层传输方式为 ws，伪装域名为发布后的域名，路径默认是 /vless/${UUID}，也可以通过 VLESS_PATH 环境变量配置。  
    d. 传输层安全为 tls，SNI 为发布后的域名，alpn 为 h2,http/1.1（这项估计不配也可以）。  

## TODO
* UDP 请求支持（只有 DNS），参考：https://github.com/zhu327/workers-tunnel/blob/main/src/lib.rs#L256  
  需要研究一下 UDP 和 DNS 消息的结构，但没找到准确的文档说明。
* 支持可选的订阅地址用来展示链接