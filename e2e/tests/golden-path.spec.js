// @ts-check
/**
 * Golden-path E2E test for Model UN.
 *
 * Validates that 1–12 players can join the same room (each in their own
 * isolated BrowserContext), cast votes, reveal results, and reset –
 * cycling through every available vote option.
 *
 * Test structure (for n = 1 to MAX_PLAYERS)
 * ──────────────────────────────────────────
 *  1. Open a fresh BrowserContext for player n (simulates a separate browser).
 *  2. Navigate to the shared room URL.
 *  3. Enter the player's name; assert it appears on the correct card on
 *     every already-connected player's page.
 *  4. For each vote option in VOTE_OPTIONS:
 *     a. Every player selects the vote value (in parallel).
 *     b. Assert all tracked cards still show "?" (hidden before reveal).
 *     c. Player n clicks "Reveal".
 *     d. Assert button flips to "Reset" and all cards show the voted value.
 *     e. Player n clicks "Reset".
 *     f. Assert button flips to "Reveal" and all cards show "?" again.
 */

const { test, expect } = require("@playwright/test");

// ── Constants ─────────────────────────────────────────────────────────────────

/** Unique room name per test run to avoid cross-run interference. */
const ROOM = `GOLDEN-PATH-${Date.now()}`;

/**
 * Every selectable vote value, matching the <option> elements in index.html.
 * The test cycles through all of them to validate each option end-to-end.
 */
const VOTE_OPTIONS = ["1", "2", "3", "5", "8", "13", "21"];

/** Maximum delegate seats (mirrors server MAX_ROOM_SIZE = 12). */
const MAX_PLAYERS = 12;

// ── Helpers ───────────────────────────────────────────────────────────────────

/**
 * Assert concurrently that every (viewPage, playerIndex) combination shows
 * `expectedText` in the player-value element.
 *
 * @param {import('@playwright/test').Page[]} viewPages
 * @param {number[]} playerIndices
 * @param {string}   expectedText
 */
async function assertAllCardsShow(viewPages, playerIndices, expectedText) {
  await Promise.all(
    viewPages.flatMap((viewPage) =>
      playerIndices.map((pi) =>
        expect(viewPage.locator(`#player${pi}value`)).toHaveText(expectedText, {
          timeout: 15_000,
        }),
      ),
    ),
  );
}

// ── Test ──────────────────────────────────────────────────────────────────────

test.setTimeout(30 * 60 * 1000); // 30 minutes for the full 12-player scenario

test(
  "Golden Path: 1 to 12 players cycle through all vote options",
  async ({ browser }) => {
    /**
     * @type {Array<{
     *   context:     import('@playwright/test').BrowserContext,
     *   page:        import('@playwright/test').Page,
     *   playerIndex: number,
     *   name:        string
     * }>}
     */
    const players = [];

    try {
      for (let n = 1; n <= MAX_PLAYERS; n++) {
        // ── 1. Open a fresh BrowserContext for player n ───────────────────────
        const context = await browser.newContext();
        const page = await context.newPage();

        // Accept any confirm dialogs (e.g. "reveal with missing votes?").
        page.on("dialog", (dialog) => dialog.accept());

        const playerName = `Player${n}`;
        const playerIndex = n - 1; // server assigns IDs starting at 0

        // ── 2. Navigate to the shared room ────────────────────────────────────
        await page.goto(`/index.html?room=${ROOM}`);

        // game.js focuses #player_name after the WebSocket handshake, so
        // waiting for focus is a reliable signal that the client is ready.
        await expect(page.locator("#player_name")).toBeFocused({
          timeout: 15_000,
        });

        // ── 3. Enter name; assert propagation to all pages ────────────────────
        await page.fill("#player_name", playerName);

        await Promise.all([
          expect(page.locator(`#player${playerIndex}name`)).toHaveText(
            playerName,
            { timeout: 10_000 },
          ),
          ...players.map(({ page: ep }) =>
            expect(ep.locator(`#player${playerIndex}name`)).toHaveText(
              playerName,
              { timeout: 10_000 },
            ),
          ),
        ]);

        players.push({ context, page, playerIndex, name: playerName });

        // ── 4. Vote cycle ─────────────────────────────────────────────────────
        // Validate from the first (oldest) page and the newest page.
        // When n=1 both references are the same; dedup avoids redundant checks.
        const firstPage = players[0].page;
        const viewPages =
          firstPage === page ? [page] : [firstPage, page];
        const playerIndices = players.map((p) => p.playerIndex);

        for (const voteValue of VOTE_OPTIONS) {
          // (a) Every player votes in parallel.
          await Promise.all(
            players.map(({ page: p }) =>
              p.selectOption("#player_value", voteValue),
            ),
          );

          // (b) Before reveal: cards must hide values.
          await assertAllCardsShow(viewPages, playerIndices, "?");

          // (c) Player n reveals.
          await page.click("#reveal-button");

          // (d) Button must say "Reset"; cards must show the voted value.
          await expect(page.locator("#reveal-button")).toHaveText("Reset", {
            timeout: 10_000,
          });
          await assertAllCardsShow(viewPages, playerIndices, voteValue);

          // (e) Player n resets.
          await page.click("#reveal-button");

          // (f) Button must say "Reveal"; cards must hide values again.
          await expect(page.locator("#reveal-button")).toHaveText("Reveal", {
            timeout: 10_000,
          });
          await assertAllCardsShow(viewPages, playerIndices, "?");
        }
      }
    } finally {
      // Close all contexts regardless of pass / fail.
      for (const { context } of players) {
        await context.close();
      }
    }
  },
);
