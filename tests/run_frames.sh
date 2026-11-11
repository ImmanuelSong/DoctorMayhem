#!/bin/sh
# tests/frames/*.png 로 감지·인식을 오프라인 확인 (WSL 에서 실행, exe 는 Windows 에서 돈다).
# 파일 이름이 기대값: none_* = 선택 화면 아님, sel_A,B,C = 카드 이름("-" = 데이터에 없는 증강).
cd "$(dirname "$0")/.." || exit 1
exe=target/x86_64-pc-windows-gnu/release/mayhem-overlay.exe
fail=0
for f in tests/frames/*.png; do
    name=$(basename "$f" .png)
    case $name in
        none_*) want="selecting=false cards=" ;;
        sel_*) want=${name#sel_}; want="selecting=true cards=${want%%_*}" ;;
    esac
    got=$("$exe" --image "$(wslpath -w "$f")" | tr -d '\r' | head -1)
    if [ "$got" = "$want" ]; then echo "ok   $name"; else echo "FAIL $name: got '$got'"; fail=1; fi
done
exit $fail
