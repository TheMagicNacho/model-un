const VOTING_SEQUENCES = Object.freeze({
  Fibonacci: Object.freeze([
    { value: 1, label: "1" },
    { value: 2, label: "2" },
    { value: 3, label: "3" },
    { value: 5, label: "5" },
    { value: 8, label: "8" },
    { value: 13, label: "13" },
    { value: 21, label: "21" },
  ]),
  Linear: Object.freeze([
    { value: 1, label: "1" },
    { value: 2, label: "2" },
    { value: 3, label: "3" },
    { value: 4, label: "4" },
    { value: 5, label: "5" },
    { value: 6, label: "6" },
    { value: 7, label: "7" },
    { value: 8, label: "8" },
    { value: 9, label: "9" },
    { value: 10, label: "10" },
  ]),
  SmMedLgXl: Object.freeze([
    { value: 1, label: "S" },
    { value: 2, label: "M" },
    { value: 3, label: "L" },
    { value: 4, label: "XL" },
  ]),
});

// Punctuation, control characters, and angle brackets are prohibited in player names.
const ILLEGAL_NAME_CHARS = /[<>]|[\p{P}\p{C}]/u;
const ILLEGAL_NAME_CHARS_GLOBAL = /[<>]|[\p{P}\p{C}]/gu;

class Game {
  constructor() {
    this.server_state = {};
    this.local_state = {
      type: "None",
      player_id: 0,
      name: "",
      value: 0,
      previous_player_size: 0,
      current_sequence: "Fibonacci",
    };
    // Values must match the server-side values
    this.max_table_size = 12;
    this.overflow_index = 100;
  }

  async run() {
    // read the room parameter from the URL
    const room_name = new URL(window.location.href).searchParams.get("room");
    const ws_protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
    const ws = await this.connect_to_server(
      `${ws_protocol}//${window.location.host}/ws/${room_name}`,
    );

    // Debaouncing is used for the field inputs to limit spamming the server.
    const debounce_time = 10;
    let debounce_timer;

    // Add event listener to name input
    const name_input = document.getElementById("player_name");
    name_input.addEventListener("input", () => {
      clearTimeout(debounce_timer);
      debounce_timer = setTimeout(() => {
        this.handle_name_change(this.local_state, ws);
      }, debounce_time);
    });
    name_input.focus();

    // Add event listener to value input
    const value_input = document.getElementById("player_value");
    value_input.addEventListener("change", () => {
      this.handle_value_change(this.local_state, ws);
    });

    // Add event listener to reveal button
    const reveal_button = document.getElementById("reveal-button");
    reveal_button.addEventListener("click", () => {
      this.handle_reveal_button_click(this.local_state, ws);
    });

    // Add click handlers to all player card slots for seat switching.
    // If the clicking player is the captain and clicks their own card, open the
    // voting-sequence popup instead of attempting a seat switch.
    for (let i = 0; i < this.max_table_size; i++) {
      const player_card = document.getElementById(`player${i}id`);
      if (player_card) {
        player_card.addEventListener("click", () => {
          if (i === this.local_state.player_id && this.is_captain()) {
            this.open_sequence_popup();
          } else {
            this.handle_seat_change(i, ws);
          }
        });
      }
    }

    // Sequence option buttons: send ChangeSequence and close popup
    document.querySelectorAll(".sequence-option").forEach((btn) => {
      btn.addEventListener("click", () => {
        ws.send(
          JSON.stringify({
            type: "ChangeSequence",
            player_id: this.local_state.player_id,
            sequence: btn.dataset.sequence,
          }),
        );
        document.getElementById("sequence-popup").style.display = "none";
      });
    });

    // Close button
    document.getElementById("sequence-popup-close").addEventListener("click", () => {
      document.getElementById("sequence-popup").style.display = "none";
    });
  }

  open_sequence_popup() {
    const popup = document.getElementById("sequence-popup");
    popup.style.display = "flex";
    // Highlight the currently active sequence
    document.querySelectorAll(".sequence-option").forEach((btn) => {
      btn.classList.toggle(
        "active",
        btn.dataset.sequence === (this.server_state.voting_sequence ?? "Fibonacci"),
      );
    });
  }

  handle_seat_change(new_seat, ws) {
    // Only switch if clicking a different, vacant seat
    if (this.local_state.player_id === new_seat) return;
    const is_vacant = !this.server_state.players?.some((p) => p.player_id === new_seat);
    if (is_vacant) {
      ws.send(
        JSON.stringify({
          type: "ChangeSeat",
          name: this.local_state.name,
          current_id: this.local_state.player_id,
          requested_id: new_seat,
        }),
      );
    }
  }

  is_captain() {
    const active_players = (this.server_state.players || []).filter(
      (p) => p.player_id < this.overflow_index,
    );
    if (active_players.length === 0) return false;
    const min_id = Math.min(...active_players.map((p) => p.player_id));
    return this.local_state.player_id === min_id;
  }

  update_vote_options(sequence_name) {
    try {
      const sequence = VOTING_SEQUENCES[sequence_name];
      const value_input = document.getElementById("player_value");
      const current_value = parseInt(value_input.value);
      value_input.replaceChildren();
      const placeholder = document.createElement("option");
      placeholder.value = "0";
      placeholder.textContent = "Select a value";
      value_input.appendChild(placeholder);
      for (const item of sequence) {
        const option = document.createElement("option");
        option.value = item.value;
        option.textContent = item.label;
        value_input.appendChild(option);
      }
      value_input.value = sequence.some((item) => item.value === current_value) ? current_value : 0;
    } catch (e) {
      console.error("Failed to update vote options:", e);
    }
  }

  get_display_label(value) {
    const sequence = VOTING_SEQUENCES[this.local_state.current_sequence];
    const item = sequence.find((item) => item.value === value);
    return item ? item.label : String(value);
  }

  handle_reveal_button_click(local_state, ws) {
    const active_players = [...this.server_state.players]
      .sort((a, b) => {
        const idA =
          a && typeof a === "object" && Object.hasOwn(a, "player_id") ? a.player_id : Infinity; // Handle missing player_id gracefully
        const idB =
          b && typeof b === "object" && Object.hasOwn(b, "player_id") ? b.player_id : Infinity;
        return idA - idB;
      })
      .slice(0, 5);
    const is_missing_votes = active_players.some(
      (obj) => obj && typeof obj === "object" && obj.value < 1,
    );

    if (this.server_state.all_revealed) {
      // Set the Select to default value
      document.getElementById("player_value").value = 0;
    }

    const request = {};
    request.type = "RevealNumbers";
    request.value = !this.server_state.all_revealed;

    if (is_missing_votes && request.value === true) {
      const user_confirmed = confirm(
        "Some delegates are missing votes.\nAre you sure you want to reveal?",
      );
      if (user_confirmed) {
        ws.send(JSON.stringify(request));
      }
    } else {
      ws.send(JSON.stringify(request));
    }
  }

  handle_value_change(local_state, ws) {
    local_state.value = parseInt(document.getElementById("player_value").value);
    const request = local_state;
    request.type = "ChangeValue";
    ws.send(JSON.stringify(request));
  }

  handle_name_change(local_state, ws) {
    const name_input = document.getElementById("player_name");
    const raw_name = name_input.value;
    const illegal_match = raw_name.match(ILLEGAL_NAME_CHARS);

    if (illegal_match) {
      alert(`Illegal character detected: "${illegal_match[0]}"`);
      name_input.value = raw_name.replace(ILLEGAL_NAME_CHARS_GLOBAL, "");
      return;
    }

    local_state.name = raw_name;
    const request = local_state;
    request.type = "ChangeName";
    ws.send(JSON.stringify(request));
  }

  async connect_to_server(server_address) {
    return new Promise((resolve, reject) => {
      const ws = new WebSocket(server_address);
      ws.onopen = () => {
        console.log("Connected to server");
        resolve(ws);
      };

      ws.onmessage = (event) => {
        const temp_state = JSON.parse(event.data);

        if (temp_state.type === "PlayerAssigned") {
          this.local_state.player_id = temp_state.player_id;
        }

        if (temp_state.type === "UpdateState") {
          this.server_state = temp_state;

          if (this.local_state.player_id === temp_state.notify_change.current_id) {
            if (temp_state.notify_change.current_id !== temp_state.notify_change.new_id) {
              console.log("Previous Player ID:", this.local_state.player_id);
              this.local_state.player_id = temp_state.notify_change.new_id;
              console.log("Updated Player ID:", this.local_state.player_id);
              this.handle_name_change(this.local_state, ws);
            }
          }
          this.update_dom_from_server_state();
        }

        if (temp_state.type === "Ping") {
          ws.send(
            JSON.stringify({
              type: "Pong",
              player_id: this.local_state.player_id,
            }),
          );
        }
      };

      ws.onerror = (error) => {
        console.error("WebSocket error:", error);
        reject(error);
      };
    });
  }

  get_captain_id() {
    if (!this.server_state.players || this.server_state.players.length === 0) return null;
    const active = this.server_state.players.filter((p) => p.player_id < this.overflow_index);
    if (active.length === 0) return null;
    return Math.min(...active.map((p) => p.player_id));
  }

  update_dom_from_server_state() {
    const current_size = this.server_state.players.length;
    const captain_id = this.get_captain_id();

    // Update vote options if the sequence changed
    const server_sequence = this.server_state.voting_sequence;
    if (server_sequence !== this.local_state.current_sequence) {
      this.local_state.current_sequence = server_sequence;
      this.update_vote_options(server_sequence);
    }

    const tableArea = document.getElementsByClassName("table-area")[0];
    if (tableArea && current_size > 6 && current_size !== this.local_state.previous_player_size) {
      tableArea.classList.add("compact");
    } else {
      tableArea.classList.remove("compact");
    }

    for (let i = 0; i < this.max_table_size; i++) {
      const player_card_element = document.getElementById(`player${i}id`);
      const player_name_element = document.getElementById(`player${i}name`);
      const player_value_element = document.getElementById(`player${i}value`);

      if (player_card_element) {
        // Hide the second row if less than 6 players
        if (i >= 6) {
          if (current_size > 6) {
            player_card_element.classList.remove("hidden-card");
          } else {
            player_card_element.classList.add("hidden-card");
          }
        }

        if (current_size > 6) {
          player_card_element.classList.add("compact");
        } else {
          player_card_element.classList.remove("compact");
        }

        // Reset state for all cards first
        player_card_element.classList.remove("player-ready");
        player_card_element.classList.add("player-vacant");
        player_card_element.classList.remove("player-captain");
        player_card_element.classList.remove("player-captain-self");

        // Find if there's a player for this position
        const player = this.server_state.players.find((p) => p.player_id === i);

        if (player) {
          // Update card with player data
          player_card_element.classList.remove("player-vacant");
          if (player.value > 0) {
            player_card_element.classList.add("player-ready");
          }
          if (player.player_id === captain_id) {
            player_card_element.classList.add("player-captain");
            // player-captain-self marks the local player's card when they are
            // captain — the CSS uses it to show cursor:pointer and a grow effect
            // on crown hover, and the click handler uses it to open the popup.
            if (player.player_id === this.local_state.player_id) {
              player_card_element.classList.add("player-captain-self");
            }
          }

          if (player_name_element) {
            player_name_element.textContent = player.player_name ?? `Player ${i}`;
          }
          if (player_value_element) {
            player_value_element.textContent =
              this.server_state.all_revealed && player.value
                ? this.get_display_label(player.value)
                : "?";
          }
        } else {
          // Reset name and value for vacant spots
          if (player_name_element) {
            player_name_element.textContent = `Delegate ${i}`;
          }
          if (player_value_element) {
            player_value_element.textContent = "?";
          }
        }
      }
    }

    // Update the player cards based on server state
    for (const player of this.server_state.players) {
      if (player.player_id !== null) {
        const player_card_id = "player" + player.player_id + "id";
        const player_name_id = "player" + player.player_id + "name";
        const player_value_id = "player" + player.player_id + "value";

        // Get elements
        const player_card_element = document.getElementById(player_card_id);
        const player_name_element = document.getElementById(player_name_id);
        const player_value_element = document.getElementById(player_value_id);

        if (player_card_element) {
          // Mark the card as occupied
          player_card_element.classList.remove("player-vacant");
          if (player.value > 0) {
            player_card_element.classList.add("player-ready");
          }
        }
        if (player_name_element) {
          // Update the name
          player_name_element.textContent = player.player_name ?? `Player ${player.player_id}`;
        }
        if (player_value_element) {
          // Update the value only if revealed
          if (this.server_state.all_revealed) {
            player_value_element.textContent = player.value
              ? this.get_display_label(player.value)
              : "?";
          } else {
            player_value_element.textContent = "?";
          }
        }
        // Update the reveal/reset button text
        const reveal_button_element = document.getElementById("reveal-button");
        if (reveal_button_element) {
          if (this.server_state.all_revealed) {
            reveal_button_element.textContent = "Reset";
          } else {
            reveal_button_element.textContent = "Reveal";
          }
        }
      }
    }

    const control_area = document.getElementById("polymorphic-hud");
    const value_input = document.getElementById("player_value");
    const reveal_button = document.getElementById("reveal-button");
    const value_label = document.querySelector('label[for="player_value"]');

    if (this.local_state.player_id >= this.overflow_index) {
      // Spectator Mode

      if (control_area) {
        if (value_input) value_input.style.visibility = "hidden";
        if (reveal_button) reveal_button.style.visibility = "hidden";
        if (value_label) value_label.style.visibility = "hidden";

        // Create and insert the Spectator Mode message
        if (!document.getElementById("spectator-message")) {
          const spectator_message = document.createElement("div");
          spectator_message.innerHTML =
            "" +
            "<h2>Spectator Mode</h2>" +
            "<p>All the delegate seats are taken.</p>" +
            "<p>When a current delegate leaves, you will automatically take their seat. In the mean time, feel free to enter your name!</p>";

          spectator_message.id = "spectator-message"; // Give it an ID if you need to manipulate it later.
          control_area.appendChild(spectator_message);
        }
      }
    } else {
      if (value_input) value_input.style.visibility = "visible";
      if (reveal_button) reveal_button.style.visibility = "visible";
      if (value_label) value_label.style.visibility = "visible";

      if (document.getElementById("spectator-message")) {
        document.getElementById("spectator-message").remove();
      }
    }
  }
}

const game = new Game();
game.run();
