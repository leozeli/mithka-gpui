/* Minimal libtdjson stand-in for the spike's receive loop.
 * Not TDLib. It checks that setTdlibParameters carries an empty encryption
 * key and then feeds a scripted authorization + chat list.
 *
 * MITHKA_STUB_MODE:
 *   (unset)      Ready, two chat titles, then close
 *   lock         400 Can't lock file
 *   encryption   401 Wrong database encryption key
 *   generation   database is from a future TDLib version
 *   phone        authorizationStateWaitPhoneNumber
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

static int phase = 0;
static char last_request[16384];
static char out[32768];

static const char *mode(void) {
    const char *value = getenv("MITHKA_STUB_MODE");
    return value == NULL ? "" : value;
}

static void sleep_secs(double timeout) {
    if (timeout <= 0) {
        return;
    }
    struct timespec ts;
    ts.tv_sec = (time_t)timeout;
    ts.tv_nsec = (long)((timeout - (double)ts.tv_sec) * 1000000000.0);
    if (ts.tv_nsec < 0) {
        ts.tv_nsec = 0;
    }
    nanosleep(&ts, NULL);
}

static int parameters_ok(const char *request) {
    return request != NULL && strstr(request, "\"@type\":\"setTdlibParameters\"") != NULL &&
           strstr(request, "\"database_encryption_key\":\"\"") != NULL &&
           strstr(request, "\"use_test_dc\":false") != NULL &&
           strstr(request, "\"use_file_database\":true") != NULL &&
           strstr(request, "\"use_chat_info_database\":true") != NULL &&
           strstr(request, "\"use_message_database\":true") != NULL &&
           strstr(request, "\"use_secret_chats\":true") != NULL;
}

int td_create_client_id(void) { return 7; }

void td_send(int client_id, const char *request) {
    (void)client_id;
    if (request == NULL) {
        last_request[0] = '\0';
        return;
    }
    snprintf(last_request, sizeof last_request, "%s", request);
}

const char *td_execute(const char *request) {
    if (request != NULL && strstr(request, "\"name\":\"version\"") != NULL) {
        return "{\"@type\":\"optionValueString\",\"name\":\"version\",\"value\":\"1.8.67-stub\"}";
    }
    return "{\"@type\":\"ok\"}";
}

const char *td_mithka_last_error(void) { return ""; }

static long json_i64_field(const char *request, const char *field) {
    char needle[64];
    const char *marker;
    snprintf(needle, sizeof needle, "\"%s\":", field);
    marker = strstr(request, needle);
    if (marker == NULL) {
        return 0;
    }
    return strtol(marker + strlen(needle), NULL, 10);
}

const char *td_receive(double timeout) {
    if (strstr(last_request, "\"@type\":\"close\"") != NULL) {
        phase = 90;
        return "{\"@type\":\"updateAuthorizationState\",\"authorization_state\":{\"@type\":\"authorizationStateClosed\"},\"@client_id\":7}";
    }

    /* Answer GTK follow-ups without disturbing the CLI phase machine.
     * td_send keeps only the latest request, so these fire when that request
     * is the one the client is waiting on. */
    if (strstr(last_request, "\"@type\":\"getMe\"") != NULL ||
        strstr(last_request, "\"@type\":\"getUser\"") != NULL) {
        last_request[0] = '\0';
        return "{\"@type\":\"user\",\"id\":7,\"first_name\":\"Ada\",\"last_name\":\"Lovelace\",\"@client_id\":7}";
    }
    if (strstr(last_request, "chatListFolder") != NULL &&
        strstr(last_request, "\"@type\":\"loadChats\"") != NULL) {
        long folder = json_i64_field(last_request, "chat_folder_id");
        snprintf(out, sizeof out,
                 "{\"@type\":\"ok\",\"@extra\":\"loadChats:folder:%ld\",\"@client_id\":7}", folder);
        last_request[0] = '\0';
        return out;
    }
    if (strstr(last_request, "\"@type\":\"loadChats\"") != NULL) {
        last_request[0] = '\0';
        return "{\"@type\":\"ok\",\"@extra\":\"loadChats\",\"@client_id\":7}";
    }
    if (strstr(last_request, "chatListFolder") != NULL &&
        strstr(last_request, "\"@type\":\"getChats\"") != NULL) {
        long folder = json_i64_field(last_request, "chat_folder_id");
        snprintf(out, sizeof out,
                 "{\"@type\":\"chats\",\"total_count\":1,\"chat_ids\":[102],\"@extra\":\"getChats:folder:%ld\",\"@client_id\":7}",
                 folder);
        last_request[0] = '\0';
        return out;
    }
    if (strstr(last_request, "\"@type\":\"viewMessages\"") != NULL ||
        strstr(last_request, "\"@type\":\"toggleChatIsMarkedAsUnread\"") != NULL) {
        long id = json_i64_field(last_request, "chat_id");
        snprintf(out, sizeof out,
                 "{\"@type\":\"updateChatReadInbox\",\"chat_id\":%ld,\"unread_count\":0,\"@client_id\":7}",
                 id);
        last_request[0] = '\0';
        return out;
    }
    if (strstr(last_request, "\"@type\":\"openChat\"") != NULL ||
        strstr(last_request, "\"@type\":\"closeChat\"") != NULL) {
        last_request[0] = '\0';
        return "{\"@type\":\"ok\",\"@client_id\":7}";
    }
    if (strstr(last_request, "getChatHistory") != NULL) {
        long id = json_i64_field(last_request, "chat_id");
        long from = json_i64_field(last_request, "from_message_id");
        if (from != 0) {
            snprintf(out, sizeof out,
                     "{\"@type\":\"messages\",\"total_count\":2,\"@extra\":\"history:%ld\",\"messages\":["
                     "{\"@type\":\"message\",\"id\":4001,\"chat_id\":%ld,\"date\":1690000000,\"sender_id\":{\"@type\":\"messageSenderChat\",\"chat_id\":%ld},\"content\":{\"@type\":\"messageText\",\"text\":{\"@type\":\"formattedText\",\"text\":\"Older line\"}}},"
                     "{\"@type\":\"message\",\"id\":4002,\"chat_id\":%ld,\"date\":1690000100,\"sender_id\":{\"@type\":\"messageSenderChat\",\"chat_id\":%ld},\"content\":{\"@type\":\"messagePhoto\",\"caption\":{\"@type\":\"formattedText\",\"text\":\"Older photo\"},\"photo\":{\"@type\":\"photo\",\"sizes\":[{\"@type\":\"photoSize\",\"type\":\"m\",\"width\":280,\"height\":180,\"photo\":{\"@type\":\"file\",\"id\":43,\"local\":{\"@type\":\"localFile\",\"path\":\"/tmp/mithka-stub-photo.png\",\"is_downloading_completed\":true,\"can_be_downloaded\":true}}},{\"@type\":\"photoSize\",\"type\":\"w\",\"width\":1280,\"height\":800,\"photo\":{\"@type\":\"file\",\"id\":45,\"local\":{\"@type\":\"localFile\",\"path\":\"/tmp/mithka-stub-photo-full.png\",\"is_downloading_completed\":true,\"can_be_downloaded\":true}}}]}}}"
                     "],\"@client_id\":7}",
                     id, id, id, id, id);
        } else {
            const char *prefix = id == 102 ? "Note from Beta " : "Hello from Alpha ";
            const char *url = "https://example.com/mithka";
            char text[160];
            snprintf(text, sizeof text, "%s%s", prefix, url);
            int n = snprintf(out, sizeof out,
                     "{\"@type\":\"messages\",\"total_count\":12,\"@extra\":\"history:%ld\",\"messages\":["
                     "{\"@type\":\"message\",\"id\":5001,\"chat_id\":%ld,\"date\":1700000000,\"sender_id\":{\"@type\":\"messageSenderChat\",\"chat_id\":%ld},\"content\":{\"@type\":\"messageText\",\"text\":{\"@type\":\"formattedText\",\"text\":\"%s\",\"entities\":[{\"@type\":\"textEntity\",\"offset\":%d,\"length\":%d,\"type\":{\"@type\":\"textEntityTypeUrl\"}}]}}},"
                     "{\"@type\":\"message\",\"id\":5003,\"chat_id\":%ld,\"date\":1700000100,\"sender_id\":{\"@type\":\"messageSenderChat\",\"chat_id\":%ld},\"content\":{\"@type\":\"messagePhoto\",\"caption\":{\"@type\":\"formattedText\",\"text\":\"A picture\"},\"photo\":{\"@type\":\"photo\",\"sizes\":[{\"@type\":\"photoSize\",\"type\":\"m\",\"width\":280,\"height\":180,\"photo\":{\"@type\":\"file\",\"id\":42,\"local\":{\"@type\":\"localFile\",\"path\":\"/tmp/mithka-stub-photo.png\",\"is_downloading_completed\":true,\"can_be_downloaded\":true}}},{\"@type\":\"photoSize\",\"type\":\"w\",\"width\":1280,\"height\":800,\"photo\":{\"@type\":\"file\",\"id\":44,\"local\":{\"@type\":\"localFile\",\"path\":\"/tmp/mithka-stub-photo-full.png\",\"is_downloading_completed\":true,\"can_be_downloaded\":true}}}]}}}",
                     id, id, id, text, (int)strlen(prefix), (int)strlen(url), id, id);
            /* Extra lines make the first page taller than the window, so a scroll-to-top is possible. */
            for (int i = 1; i <= 10 && n > 0 && (size_t)n < sizeof out; i++) {
                n += snprintf(out + n, sizeof out - (size_t)n,
                              ",{\"@type\":\"message\",\"id\":%d,\"chat_id\":%ld,\"date\":%d,\"sender_id\":{\"@type\":\"messageSenderChat\",\"chat_id\":%ld},\"content\":{\"@type\":\"messageText\",\"text\":{\"@type\":\"formattedText\",\"text\":\"History line %d\"}}}",
                              5100 + i, id, 1700000200 + i, id, i);
            }
            if (n > 0 && (size_t)n < sizeof out) {
                n += snprintf(out + n, sizeof out - (size_t)n,
                              ",{\"@type\":\"message\",\"id\":4900,\"chat_id\":%ld,\"date\":1699999000,\"is_outgoing\":true,\"sender_id\":{\"@type\":\"messageSenderUser\",\"user_id\":7},\"content\":{\"@type\":\"messageText\",\"text\":{\"@type\":\"formattedText\",\"text\":\"Seen by Ada\"}}}",
                              id);
            }
            if (n > 0 && (size_t)n < sizeof out) {
                n += snprintf(out + n, sizeof out - (size_t)n,
                              ",{\"@type\":\"message\",\"id\":5200,\"chat_id\":%ld,\"date\":1700003000,\"is_outgoing\":true,\"sender_id\":{\"@type\":\"messageSenderUser\",\"user_id\":7},\"content\":{\"@type\":\"messageText\",\"text\":{\"@type\":\"formattedText\",\"text\":\"Not seen yet\"}}}",
                              id);
            }
            if (n > 0 && (size_t)n < sizeof out) {
                snprintf(out + n, sizeof out - (size_t)n, "],\"@client_id\":7}");
            }
        }
        last_request[0] = '\0';
        return out;
    }
    if (strstr(last_request, "\"@type\":\"sendMessage\"") != NULL) {
        long id = json_i64_field(last_request, "chat_id");
        char text[512];
        const char *marker = strstr(last_request, "\"text\":\"");
        text[0] = '\0';
        if (marker != NULL) {
            marker += 8;
            size_t n = 0;
            while (marker[n] != '\0' && marker[n] != '"' && n + 1 < sizeof text) {
                text[n] = marker[n];
                n++;
            }
            text[n] = '\0';
        }
        if (text[0] == '\0') {
            snprintf(text, sizeof text, "Sent");
        }
        snprintf(out, sizeof out,
                 "{\"@type\":\"message\",\"id\":5002,\"chat_id\":%ld,\"date\":1700001000,\"is_outgoing\":true,\"sender_id\":{\"@type\":\"messageSenderUser\",\"user_id\":7},\"content\":{\"@type\":\"messageText\",\"text\":{\"@type\":\"formattedText\",\"text\":\"%s\"}},\"@client_id\":7}",
                 id, text);
        last_request[0] = '\0';
        return out;
    }

    if (phase == 0) {
        phase = 1;
        return "{\"@type\":\"updateAuthorizationState\",\"authorization_state\":{\"@type\":\"authorizationStateWaitTdlibParameters\"},\"@client_id\":7}";
    }

    if (phase == 1 && strstr(last_request, "setTdlibParameters") != NULL) {
        if (!parameters_ok(last_request)) {
            phase = 50;
            return "{\"@type\":\"error\",\"code\":400,\"message\":\"stub rejected setTdlibParameters\",\"@extra\":\"setTdlibParameters\",\"@client_id\":7}";
        }
        phase = 50;
        if (strcmp(mode(), "lock") == 0) {
            return "{\"@type\":\"error\",\"code\":400,\"message\":\"Can't lock file \\\"/tmp/td.binlog\\\"\",\"@extra\":\"setTdlibParameters\",\"@client_id\":7}";
        }
        if (strcmp(mode(), "encryption") == 0) {
            return "{\"@type\":\"error\",\"code\":401,\"message\":\"Wrong database encryption key\",\"@extra\":\"setTdlibParameters\",\"@client_id\":7}";
        }
        if (strcmp(mode(), "generation") == 0) {
            return "{\"@type\":\"error\",\"code\":400,\"message\":\"database is from a future TDLib version\",\"@extra\":\"setTdlibParameters\",\"@client_id\":7}";
        }
        if (strcmp(mode(), "phone") == 0) {
            return "{\"@type\":\"updateAuthorizationState\",\"authorization_state\":{\"@type\":\"authorizationStateWaitPhoneNumber\"},\"@client_id\":7}";
        }
        phase = 2;
        return "{\"@type\":\"updateAuthorizationState\",\"authorization_state\":{\"@type\":\"authorizationStateReady\"},\"@client_id\":7}";
    }

    if (phase == 2 && strstr(last_request, "getChats") != NULL) {
        phase = 3;
        return "{\"@type\":\"updateNewChat\",\"chat\":{\"@type\":\"chat\",\"id\":101,\"title\":\"Alpha\",\"unread_count\":4,\"last_read_outbox_message_id\":4900,\"last_message\":{\"@type\":\"message\",\"id\":5001,\"chat_id\":101,\"date\":1700000000,\"content\":{\"@type\":\"messageText\",\"text\":{\"@type\":\"formattedText\",\"text\":\"Hello from Alpha\"}}},\"photo\":{\"@type\":\"chatPhotoInfo\",\"small\":{\"@type\":\"file\",\"id\":11,\"local\":{\"@type\":\"localFile\",\"path\":\"/tmp/mithka-stub-avatar.png\",\"is_downloading_completed\":true,\"can_be_downloaded\":true}}}},\"@client_id\":7}";
    }

    if (phase == 3 && strstr(last_request, "getChats") != NULL) {
        phase = 4;
        return "{\"@type\":\"chats\",\"total_count\":2,\"chat_ids\":[101,102],\"@extra\":\"getChats\",\"@client_id\":7}";
    }

    if (phase == 4 && strstr(last_request, "getChat") != NULL) {
        const char *marker = strstr(last_request, "\"chat_id\":");
        long id = marker == NULL ? 0 : strtol(marker + 10, NULL, 10);
        const char *title = id == 102 ? "Beta" : "Chat";
        const char *preview = id == 102 ? "Note from Beta" : "Hello from Alpha";
        const char *folder_pos = id == 102
            ? ",{\"@type\":\"chatPosition\",\"list\":{\"@type\":\"chatListFolder\",\"chat_folder_id\":2},\"order\":\"30\"}"
            : "";
        phase = 5;
        snprintf(out, sizeof out,
                 "{\"@type\":\"chat\",\"id\":%ld,\"title\":\"%s\",\"unread_count\":0,\"last_read_outbox_message_id\":4900,\"last_message\":{\"@type\":\"message\",\"id\":5001,\"chat_id\":%ld,\"date\":1700000200,\"content\":{\"@type\":\"messageText\",\"text\":{\"@type\":\"formattedText\",\"text\":\"%s\"}}},\"positions\":[{\"@type\":\"chatPosition\",\"list\":{\"@type\":\"chatListMain\"},\"order\":\"10\"}%s],\"@client_id\":7}",
                 id, title, id, preview, folder_pos);
        return out;
    }

    /* TDLib 1.8.67 has no getChatFolders. Folders arrive as updateChatFolders. */
    if (phase == 5) {
        phase = 6;
        return "{\"@type\":\"updateChatFolders\",\"chat_folders\":[{\"@type\":\"chatFolderInfo\",\"id\":2,\"name\":{\"@type\":\"chatFolderName\",\"text\":{\"@type\":\"formattedText\",\"text\":\"Work\"},\"animate_custom_emoji\":false},\"color_id\":-1,\"is_shareable\":false,\"has_my_invite_links\":false}],\"main_chat_list_position\":0,\"are_tags_enabled\":false,\"@client_id\":7}";
    }

    sleep_secs(timeout);
    return NULL;
}
