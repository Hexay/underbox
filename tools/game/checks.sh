# tools/game.sh's PASS/FAIL checks per MODE (sourced after report.sh; LOG is the server's log, or the
# host client's).
CLIENT=$OUT/client.log
case $MODE in
    server)
        compile_clean "$LOG" server
        check "player stands" 'GAME:STANDING .*on=[^ ]+' "$LOG"
        check "end reached" 'GAME:END' "$LOG"
        ;;
    join)
        compile_clean "$LOG" server
        check "client found the package offline" 'using what we knew about [^ ]+' "$CLIENT"
        refute "no package lookup failure" "wasn.t found|Unable to download package" "$CLIENT"
        check "server accepted the client" '[^]]+\] is connected' "$LOG"
        check "server sees the client's player standing" 'GAME:PLAYER_STANDING .*local=False.*' "$LOG"
        check "client's own player stands" 'GAME:PLAYER_STANDING .*local=True.*' "$CLIENT"
        ;;
    grubs)
        compile_clean "$LOG" client
        check "round started" 'GAME:COMMAND .*' "$LOG"
        check "sdflayer resources loaded" 'GAME:RESOURCES .*sdflayer=[1-9][0-9]*' "$LOG"
        check "geq resources loaded" 'GAME:RESOURCES .*geq=[1-9][0-9]*' "$LOG"
        check "terrain built" 'GAME:COMPONENTS .*Sdf2DChunk=[1-9][0-9]*' "$LOG"
        check "terrain has collision" 'GAME:PHYSICS bodies=[0-9]+ shapes=[1-9][0-9]*' "$LOG"
        check "grubs spawned" 'GAME:COMPONENTS .* Grub=[1-9][0-9]*' "$LOG"
        check "equipment spawned" 'GAME:COMPONENTS .* Equipment=[1-9][0-9]*' "$LOG"
        check "end reached" 'GAME:END' "$LOG"
        ;;
    host|render)
        compile_clean "$LOG" client
        check "end reached" 'GAME:END' "$LOG"
        ;;
    play)
        check "client window on X11" 'window: SDL video driver x11' "$OUT/client.out"
        check "input driven" 'xinput: hold:w:[0-9.]+' "$OUT/xinput.log"
        check "managed saw input" 'GAME:INPUT .*' "$CLIENT"
        check "client view turned" 'GAME:CAMERA_TURNED .*' "$CLIENT"
        if [ "$PLAY_VIA" = host ]; then
            check "client view moved" 'GAME:CAMERA_MOVED .*' "$CLIENT"
        else
            check "server sees the player move" 'GAME:PLAYER_MOVED .*local=False.*' "$LOG"
            check "server sees the player turn" 'GAME:PLAYER_TURNED .*local=False.*' "$LOG"
        fi
        ;;
    sound)
        compile_clean "$LOG" client
        check "audio device open" 'audio device: [^,]+, [0-9]+ channels' "$OUT/client.out"
        check "voices started" 'audio stats: voices=[1-9][0-9]*' "$OUT/client.out"
        check "voices carried signal" 'audio stats: .*audible_voice_blocks=[1-9][0-9]*' "$OUT/client.out"
        check "audible blocks reached the device" 'audio stats: .*audible_output_blocks=[1-9][0-9]*' "$OUT/client.out"
        ;;
esac
