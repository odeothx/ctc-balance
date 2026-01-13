# CTC Balance Tracker: Rust 마이그레이션 계획

## 개요

현재 Python으로 작성된 CTC Balance Tracker를 Rust로 완전히 재구현합니다.
Python 코드는 그대로 유지하며, 새로운 Rust 프로젝트를 `ctc-balance-rs/` 디렉토리에 생성합니다.

## 프로젝트 구조

```
ctc-balance/                    # 기존 Python 프로젝트 (유지)
├── main.py
├── accounts.py
├── src/
│   ├── chain.py
│   ├── balance.py
│   └── utils.py
└── output/

ctc-balance-rs/                 # 새 Rust 프로젝트
├── Cargo.toml
├── build.rs                    # subxt 메타데이터 빌드
├── src/
│   ├── main.rs                 # CLI 엔트리포인트
│   ├── lib.rs                  # 라이브러리 루트
│   ├── chain.rs                # ChainConnector
│   ├── balance.rs              # BalanceTracker
│   ├── accounts.rs             # 계정 파일 파싱
│   ├── cache.rs                # block_cache.json 관리
│   ├── csv_output.rs           # CSV 저장
│   └── plot.rs                 # 그래프 생성 (plotters)
├── metadata/
│   └── creditcoin3.scale       # 체인 메타데이터
└── output/                     # 결과물 디렉토리
```

## 핵심 의존성 (Cargo.toml)

```toml
[package]
name = "ctc-balance"
version = "0.1.0"
edition = "2021"

[dependencies]
# Substrate RPC
subxt = "0.35"
subxt-signer = "0.35"

# Async runtime
tokio = { version = "1", features = ["full"] }

# CLI
clap = { version = "4", features = ["derive"] }

# Serialization
serde = { version = "1", features = ["derive"] }
serde_json = "1"

# Date/Time
chrono = { version = "0.4", features = ["serde"] }

# CSV
csv = "1"

# Graph
plotters = "0.3"

# Error handling
anyhow = "1"
thiserror = "1"

# Parallel processing
rayon = "1"
futures = "0.3"

[build-dependencies]
subxt-codegen = "0.35"
```

---

## 단계별 구현 계획

### Phase 1: 프로젝트 초기화 및 체인 연결 기초 ✅ 완료

**목표:** Rust 프로젝트 생성, Creditcoin3 노드 연결 확인

**작업 내용:**
1. `ctc-balance-rs/` 디렉토리에 Cargo 프로젝트 생성
2. `Cargo.toml`에 기본 의존성 추가
3. 메타데이터 다운로드 (`subxt metadata`)
4. 기본 연결 테스트 (`get_chain_info` 구현)

**산출물:**
- `Cargo.toml`
- `src/main.rs` - 기본 연결 테스트
- `metadata/creditcoin3.scale`

---

### Phase 2: ChainConnector 핵심 기능 ✅ 완료

**목표:** 블록 조회 및 타임스탬프 기반 블록 검색 구현

**작업 내용:**
1. `src/chain.rs` 생성
2. `ChainConnector` 구조체 구현:
   - `new()` - 연결 생성
   - `get_block_hash(block_number)` - 블록 해시 조회
   - `get_block_timestamp(block_hash)` - 타임스탬프 조회
   - `get_latest_block_number()` - 최신 블록 번호
   - `find_block_at_timestamp(timestamp)` - 이진 탐색

**산출물:**
- `src/chain.rs`
- `src/lib.rs` (모듈 선언)

---

### Phase 3: 잔고 조회 기능 ✅ 완료

**목표:** 계정 잔고 조회 구현

**작업 내용:**
1. `src/balance.rs` 생성
2. `Balance` 구조체 정의 (free, reserved, frozen)
3. `BalanceTracker` 구현:
   - `get_balance(address, block_hash)` - 단일 계정 조회
   - `get_all_balances(accounts, block_hash)` - 다중 계정 조회

**산출물:**
- `src/balance.rs`

---

### Phase 4: 계정 파일 파싱 및 캐시 ✅ 완료

**목표:** 계정 파일 로드 및 블록 캐시 관리

**작업 내용:**
1. `src/accounts.rs` 생성:
   - `load_accounts(file_path)` - txt 파일 파싱
   - `Name = Address` 및 `Name Address` 형식 지원
2. `src/cache.rs` 생성:
   - `load_block_cache()` - JSON 로드
   - `save_block_cache()` - JSON 저장
   - Python과 동일한 형식 유지 (호환성)

**산출물:**
- `src/accounts.rs`
- `src/cache.rs`

---

### Phase 5: CLI 및 메인 로직 ✅ 완료

**목표:** 전체 CLI 구현 및 잔고 히스토리 조회

**작업 내용:**
1. `clap`으로 CLI 구현:
   - `-f, --file` - 계정 파일
   - `-a, --address` - 단일 주소
   - `-n, --name` - 지갑 이름
   - `--start` - 시작 날짜
   - `--end` - 종료 날짜
   - `-o, --output` - 출력 파일
   - `-g, --graph` - 그래프 생성
   - `--no-cache` - 캐시 무시
2. 날짜 범위 생성 및 병렬 처리:
   - 블록 탐색: 순차 처리
   - 잔고 조회: 순차 처리

**산출물:**
- `src/main.rs` (완성)

---

### Phase 6: CSV 출력 ✅ 완료

**목표:** CSV 파일 생성 (통합 + 개별)

**작업 내용:**
1. `src/csv_output.rs` 생성:
   - `save_combined_csv()` - 통합 CSV (Python과 동일 형식)
   - `save_individual_csvs()` - 개별 계정 CSV
2. 컬럼: date, balance, diff, diff_avg10
3. 기존 CSV 로드 및 병합 로직

**산출물:**
- `src/csv_output.rs`
- `output/xxx_history.csv`
- `output/individual/*.csv`

---

### Phase 7: 그래프 생성 ✅ 완료

**목표:** plotters를 사용한 그래프 생성

**작업 내용:**
1. `src/plot.rs` 생성:
   - `plot_balances()` - 메인 그래프 (개별 + 총합)
   - 개별 계정 그래프 자동 생성
2. 구현 세부사항:
   - 2-패널 레이아웃 (상단: 개별, 하단: 총합)
   - 월별 X축 틱
   - PNG 출력

**산출물:**
- `src/plot.rs`
- `output/xxx_history.png`
- `output/individual/*.png`

---

### Phase 8: 테스트 및 최적화 (보류)

**목표:** 안정성 확보 및 성능 최적화

**남은 작업:**
1. 에러 핸들링 강화
2. 단위 테스트 작성
3. 통합 테스트 (Python 결과와 비교)
4. 성능 벤치마크
5. 병렬 처리 구현 (tokio 활용)

---

## 예상 일정

| Phase | 작업 | 예상 소요 |
|-------|------|----------|
| 1 | 프로젝트 초기화 및 연결 | 0.5일 |
| 2 | ChainConnector | 1일 |
| 3 | 잔고 조회 | 0.5일 |
| 4 | 계정 파싱 및 캐시 | 0.5일 |
| 5 | CLI 및 메인 로직 | 1.5일 |
| 6 | CSV 출력 | 0.5일 |
| 7 | 그래프 생성 | 1일 |
| 8 | 테스트 및 최적화 | 1일 |
| **총합** | | **6.5일** |

---

## 참고: Python vs Rust 함수 매핑

| Python | Rust | Phase |
|--------|------|-------|
| `ChainConnector.__init__` | `ChainConnector::new()` | 2 |
| `ChainConnector.get_block_hash` | `ChainConnector::get_block_hash()` | 2 |
| `ChainConnector.find_block_at_timestamp` | `ChainConnector::find_block_at_timestamp()` | 2 |
| `BalanceTracker.get_balance` | `BalanceTracker::get_balance()` | 3 |
| `load_accounts` | `accounts::load()` | 4 |
| `load_block_cache` | `cache::load()` | 4 |
| `parse_args` | `clap` derive macro | 5 |
| `main` | `main()` + `run()` | 5 |
| CSV 저장 | `csv_output::save()` | 6 |
| `plot_balances` | `plot::draw()` | 7 |

---

## 다음 단계

Phase 1부터 시작하시겠습니까?
