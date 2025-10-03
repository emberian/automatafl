#!/bin/bash
set -e  # Exit on any error

# Colors for output
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m' # No Color

# Config
CLI="cargo run -p automatafl-backend-client --bin automatafl-client --"
BASE_URL="http://localhost:3000"
TEST_USER1="cli_test_user_$$_1"
TEST_USER2="cli_test_user_$$_2"
TEST_PASS="testpass123"

echo -e "${BLUE}=== AutomataFL Backend CLI Integration Tests ===${NC}\n"

# Helper function
run_test() {
    echo -e "${YELLOW}TEST:${NC} $1"
    shift
    if "$@"; then
        echo -e "${GREEN}✓ PASS${NC}\n"
        return 0
    else
        echo -e "${RED}✗ FAIL${NC}\n"
        exit 1
    fi
}

# 1. Health Check
run_test "Health check" \
    $CLI --url "$BASE_URL" health

# 2. Register User 1
run_test "Register user 1" \
    $CLI --url "$BASE_URL" register "$TEST_USER1" "$TEST_PASS"

# 3. Register User 2
run_test "Register user 2" \
    $CLI --url "$BASE_URL" register "$TEST_USER2" "$TEST_PASS"

# 4. Duplicate registration should fail
echo -e "${YELLOW}TEST:${NC} Duplicate registration (should fail)"
if $CLI --url "$BASE_URL" register "$TEST_USER1" "$TEST_PASS" 2>/dev/null; then
    echo -e "${RED}✗ FAIL - should have failed${NC}\n"
    exit 1
else
    echo -e "${GREEN}✓ PASS - correctly rejected${NC}\n"
fi

# 5. Login User 1
run_test "Login user 1" \
    $CLI --url "$BASE_URL" login "$TEST_USER1" "$TEST_PASS"

# Store user 1's player ID
PLAYER1_ID=$(grep "Player ID:" ~/.config/automatafl-client/session.txt 2>/dev/null | cut -d: -f2 | xargs || echo "")

# 6. List games (should be empty initially or contain games)
run_test "List games" \
    $CLI --url "$BASE_URL" game list

# 7. Create a game
echo -e "${YELLOW}TEST:${NC} Create game"
GAME_OUTPUT=$($CLI --url "$BASE_URL" game create --players 2 --column-rule)
GAME_ID=$(echo "$GAME_OUTPUT" | grep "Game ID:" | awk '{print $3}')
if [ -z "$GAME_ID" ]; then
    echo -e "${RED}✗ FAIL - no game ID${NC}\n"
    exit 1
fi
echo -e "${GREEN}✓ PASS - Game created: $GAME_ID${NC}\n"

# 8. Get game state
run_test "Get game state" \
    $CLI --url "$BASE_URL" game state "$GAME_ID"

# 9. Get game goals
run_test "Get game goals" \
    $CLI --url "$BASE_URL" game goals "$GAME_ID"

# 10. Logout user 1
run_test "Logout user 1" \
    $CLI --url "$BASE_URL" logout

# 11. Login user 2
run_test "Login user 2" \
    $CLI --url "$BASE_URL" login "$TEST_USER2" "$TEST_PASS"

# Store user 2's player ID
PLAYER2_OUTPUT=$($CLI --url "$BASE_URL" login "$TEST_USER2" "$TEST_PASS" 2>&1 || echo "")
PLAYER2_ID=$(echo "$PLAYER2_OUTPUT" | grep "Player ID:" | awk '{print $3}')

# 12. Join the game
run_test "Join game (user 2)" \
    $CLI --url "$BASE_URL" game join "$GAME_ID"

# 13. Perform a move
run_test "Perform move (user 2)" \
    $CLI --url "$BASE_URL" move do "$GAME_ID" 5 6 5 7

# 14. Check pending move
run_test "Check pending move" \
    $CLI --url "$BASE_URL" move pending "$GAME_ID"

# 15. Send chat message
run_test "Send chat message" \
    $CLI --url "$BASE_URL" chat send "$GAME_ID" Hello from CLI test!

# 16. Get chat history
run_test "Get chat history" \
    $CLI --url "$BASE_URL" chat history "$GAME_ID"

# 17. Get game history
run_test "Get game history" \
    $CLI --url "$BASE_URL" history "$GAME_ID"

# 18. Get filtered game history
run_test "Get filtered game history" \
    $CLI --url "$BASE_URL" history "$GAME_ID" --kind PLAYER_JOINED

# 19. Save game
run_test "Save game" \
    $CLI --url "$BASE_URL" game save "$GAME_ID"

# 20. List snapshots
run_test "List snapshots" \
    $CLI --url "$BASE_URL" game snapshots "$GAME_ID"

# 21. Load game from snapshot
run_test "Load snapshot" \
    $CLI --url "$BASE_URL" game load "$GAME_ID" 0

# 22. Get player profile
if [ -n "$PLAYER2_ID" ]; then
    run_test "Get player profile" \
        $CLI --url "$BASE_URL" profile get "$PLAYER2_ID"
else
    echo -e "${YELLOW}SKIP: Get player profile (no player ID)${NC}\n"
fi

# 23. Update player profile
if [ -n "$PLAYER2_ID" ]; then
    run_test "Update player profile" \
        $CLI --url "$BASE_URL" profile update "$PLAYER2_ID" --bio "CLI test bio"
else
    echo -e "${YELLOW}SKIP: Update player profile (no player ID)${NC}\n"
fi

# 24. Get player stats
if [ -n "$PLAYER2_ID" ]; then
    run_test "Get player stats" \
        $CLI --url "$BASE_URL" profile stats "$PLAYER2_ID"
else
    echo -e "${YELLOW}SKIP: Get player stats (no player ID)${NC}\n"
fi

# 25. Get ELO leaderboard
run_test "Get ELO leaderboard" \
    $CLI --url "$BASE_URL" leaderboard elo

# 26. Get wins leaderboard
run_test "Get wins leaderboard" \
    $CLI --url "$BASE_URL" leaderboard wins

# 27. Get games leaderboard
run_test "Get games leaderboard" \
    $CLI --url "$BASE_URL" leaderboard games

# 28. Join matchmaking
run_test "Join matchmaking" \
    $CLI --url "$BASE_URL" matchmaking join --players 2 --column-rule

# 29. Check matchmaking status
run_test "Check matchmaking status" \
    $CLI --url "$BASE_URL" matchmaking status

# 30. Leave matchmaking
run_test "Leave matchmaking" \
    $CLI --url "$BASE_URL" matchmaking leave

# 31. Verify not in queue
echo -e "${YELLOW}TEST:${NC} Verify left matchmaking queue"
QUEUE_STATUS=$($CLI --url "$BASE_URL" matchmaking status 2>&1)
if echo "$QUEUE_STATUS" | grep -q "Not in matchmaking queue"; then
    echo -e "${GREEN}✓ PASS${NC}\n"
else
    echo -e "${RED}✗ FAIL - still in queue${NC}\n"
    exit 1
fi

# 32. Unauthorized access test
run_test "Logout before unauthorized test" \
    $CLI --url "$BASE_URL" logout

echo -e "${YELLOW}TEST:${NC} Unauthorized access (should fail)"
if $CLI --url "$BASE_URL" game list 2>/dev/null; then
    echo -e "${RED}✗ FAIL - should have been unauthorized${NC}\n"
    exit 1
else
    echo -e "${GREEN}✓ PASS - correctly unauthorized${NC}\n"
fi

echo -e "${GREEN}=== All tests passed! ===${NC}"
