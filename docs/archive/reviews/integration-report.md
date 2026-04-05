# CogKOS 集成验证报告

**生成时间**: 2026-03-11 02:48

---

## 测试结果

| 指标 | 数值 |
|------|------|
| **通过** | 82 |
| **失败** | 9 |
| **忽略** | 0 |

---

## 失败的测试（9个）

| 测试名称 | 原因 | 状态 |
|----------|------|------|
| test_calculate_decay_basic | 断言阈值问题 | 需修复 |
| test_direct_contradiction | 测试数据问题 | 需修复 |
| test_source_disagreement | 阈值问题 | 需修复 |
| test_model_switch | 训练数据不足 | 需修复 |
| test_data_sufficiency_* | 数据不足 | 需修复 |
| test_needs_retraining | 数据不足 | 需修复 |
| test_training_queue | 数据不足 | 需修复 |
| test_llm_sandbox | Sandbox 问题 | 需修复 |
| test_paradigm_shift_engine | 阈值问题 | 需修复 |

---

## 功能状态

✅ **核心功能编译通过**  
✅ **82个测试通过**  
⚠️ **9个测试需修复**

---

## 下一步

1. 修复 9 个失败的测试
2. 运行完整 CI 验证
3. 准备发布

---

**阶段4：集成验证 - 完成**
