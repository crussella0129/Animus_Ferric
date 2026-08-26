# Evaluation Artifact Archive

Executable artifacts are intentionally excluded from the tracked Sprint Loops
Book. They remain in repository-local, gitignored storage; each result row
binds the frozen executable by path, byte length, and SHA-256.

| Artifact | Bytes | SHA-256 |
| --- | ---: | --- |
| `ferric-control-cabe236.exe` | 13,771,776 | `F6E636F80AD3AF22920C91A22AB0C5A1F0F4E8AFE56DFECEE77822061C8320F4` |
| Screen 002 unchanged mechanism | 19,577,856 | `F0B3D39DFFA5EE8BAA366F484A81412AA2D5F31E16280D855A0DF28127A9FFDB` |
| Screen 003 revision 1 | 21,088,256 | `0565844CB61E83683F5DF08E9345730FDE1EA47676BADBDFADB5406C8D5B380A` |
| Screen 004 revision 2 | 21,127,168 | `F98C4875BC272B8C17B26E3DDA1F5D414AE3E23E03514319DDA06A2801708F53` |

The complete structured output of every development-screen attempt is tracked
under [`evidence-screens/`](evidence-screens/): results, summaries, and retained
traces. Screen 001 is preserved as an excluded incomplete preflight rather than
silently discarded. Screens 002–004 are the three scoreable, fixed-coordinate
screens. A source-to-archive audit compared all 20 files and their bytes and
reported no mismatch.

| Screen | Run | Results SHA-256 | Summary SHA-256 | Status |
| --- | --- | --- | --- | --- |
| 001 | `autonomy-1787769525603-26616-0` | `ea823b2afb27c149f006aff301e6b7511b4dfdd296c4f1ed828499f567904336` | `9cc98de98c041cf54d6e1cdb71a4e45f2898d05a64164413655bac9c75d6b850` | excluded: only 2 of 3 result rows persisted |
| 002 | `autonomy-1787772736346-25036-0` | `7da346b5a933061915932163509e8e0743a358cb2c5336ba26d63c8ca3509775` | `718d7ce3aecba1909042ee12496d52ea27710012701e27bdd5623f39f32b9ccb` | scoreable 0/3 |
| 003 | `autonomy-1787776690194-46504-0` | `8f400dabe5734f84e79a8e086325e90c3be6485501e90180b3a978cb16253c57` | `b97617ab468b0a69dbb8b32eaf439b4f2ab631ccda79dd628b9c0217cc21c47e` | scoreable 0/3 |
| 004 | `autonomy-1787781412661-27096-0` | `094e21fa2a43c17e40df03a96877f7bf77db95644cade24dce80f0b05310e94b` | `2f6d6fb1d6e117b335ee9f693de4f5389f86884ae97343a8f58d6c676c5d285d` | scoreable 0/3; final revision budget exhausted |
