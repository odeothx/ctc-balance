# CTC Balance Tracker - Rust Implementation

Creditcoin3 지갑 잔고를 추적하는 Rust 구현체입니다.

## 빌드

```bash
cd ctc-balance-rs
cargo build --release
```

## 사용법

```bash
# 계정 파일 사용
cargo run --release -- -f ../example_accounts.txt

# 그래프 생성 포함
cargo run --release -- -f ../example_accounts.txt --graph

# 단일 지갑
cargo run --release -- -a 5DDYL8H9sVhVS3P17TBiPMZVKt4GEc24G37ov4xXykvdBDTs -n MyWallet

# 날짜 범위 지정
cargo run --release -- -f ../my_accounts.txt --start 2024-10-01 --end 2024-12-31
```

## 옵션

| 옵션 | 설명 |
|------|------|
| `-f, --file` | 계정 파일 경로 |
| `-a, --address` | 단일 지갑 주소 |
| `-n, --name` | 지갑 이름 (기본값: wallet) |
| `--start` | 시작 날짜 (YYYY-MM-DD) |
| `--end` | 종료 날짜 (YYYY-MM-DD) |
| `-o, --output` | 출력 CSV 파일 |
| `-g, --graph` | 그래프 생성 |
| `--no-cache` | 블록 캐시 무시 |

## 출력

- `output/<source>_history.csv` - 통합 잔고 히스토리
- `output/individual/<account>.csv` - 개별 계정 히스토리
- `output/<source>_history.png` - 메인 그래프
- `output/individual/<account>.png` - 개별 그래프

## 프로젝트 구조

```
src/
├── main.rs         # CLI 엔트리포인트
├── lib.rs          # 라이브러리 루트
├── chain.rs        # ChainConnector (RPC 연결)
├── balance.rs      # BalanceTracker (잔고 조회)
├── accounts.rs     # 계정 파일 파싱
├── cache.rs        # 블록 캐시 관리
├── csv_output.rs   # CSV 출력
└── plot.rs         # 그래프 생성
```

## Python 버전과의 호환성

- 동일한 계정 파일 형식 지원 (`Name = Address`, `Name Address`)
- 동일한 블록 캐시 파일 형식 (`block_cache.json`)
- 동일한 CSV 출력 형식
