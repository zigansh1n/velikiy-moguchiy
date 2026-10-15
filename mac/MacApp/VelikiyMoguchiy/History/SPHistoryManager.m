#import "SPHistoryManager.h"
#import "SPAppPaths.h"
#import <sqlite3.h>
#import <sys/stat.h>
#import <errno.h>
#import <string.h>

@implementation SPHistoryStats
@end

@interface SPHistoryManager () {
    sqlite3 *_db;
}
@end

NSNotificationName const SPHistoryDidChangeNotification = @"SPHistoryDidChangeNotification";
NSString *const SPHistoryPurposeDictate = @"dictate";
NSString *const SPHistoryPurposeTranslate = @"translate";
NSString *const SPHistoryPurposeAsk = @"ask";
NSString *const SPHistoryPurposeEdit = @"edit";

/// sp_contains(haystack, needle): 1 when `needle` occurs in `haystack`
/// ignoring case and diacritics. SQLite's own LIKE folds ASCII only, so it
/// would miss «Привет» for «привет».
static void sqliteContains(sqlite3_context *context, int argc, sqlite3_value **argv) {
    const unsigned char *haystack = sqlite3_value_text(argv[0]);
    const unsigned char *needle = sqlite3_value_text(argv[1]);
    if (!haystack || !needle) {
        sqlite3_result_int(context, 0);
        return;
    }
    NSString *text = [NSString stringWithUTF8String:(const char *)haystack];
    NSString *query = [NSString stringWithUTF8String:(const char *)needle];
    BOOL found = text && query.length > 0 &&
        [text rangeOfString:query
                    options:NSCaseInsensitiveSearch | NSDiacriticInsensitiveSearch].location != NSNotFound;
    sqlite3_result_int(context, found ? 1 : 0);
}

@implementation SPHistoryManager

+ (instancetype)sharedManager {
    static SPHistoryManager *instance;
    static dispatch_once_t onceToken;
    dispatch_once(&onceToken, ^{
        instance = [[SPHistoryManager alloc] init];
    });
    return instance;
}

- (instancetype)init {
    NSString *dir = SPAppDataDirectory();
    return [self initWithDatabasePath:[dir stringByAppendingPathComponent:@"history.db"]];
}

- (instancetype)initWithDatabasePath:(NSString *)dbPath {
    self = [super init];
    if (self) {
        [self openDatabaseAtPath:dbPath];
    }
    return self;
}

- (void)openDatabaseAtPath:(NSString *)dbPath {
    // The database stores everything the user has dictated — owner-only
    // permissions on the directory and file (matches the config hardening).
    [[NSFileManager defaultManager] createDirectoryAtPath:dbPath.stringByDeletingLastPathComponent
                              withIntermediateDirectories:YES
                                               attributes:@{NSFilePosixPermissions : @(0700)}
                                                    error:nil];

    if (sqlite3_open(dbPath.UTF8String, &_db) != SQLITE_OK) {
        // sqlite3_open returns a handle even on failure; it must be closed
        // or it (and its resources) leak until process exit.
        NSLog(@"[VelikiyMoguchiy] Failed to open history database: %s", sqlite3_errmsg(_db));
        if (_db) {
            sqlite3_close_v2(_db);
        }
        _db = NULL;
        return;
    }
    // The database holds every transcript; a failure to make it owner-only
    // is worth surfacing rather than silently continuing world-readable.
    if (chmod(dbPath.UTF8String, 0600) != 0) {
        NSLog(@"[VelikiyMoguchiy] WARNING: could not restrict history database permissions (%s): %s",
              dbPath.UTF8String, strerror(errno));
    }

    const char *sql =
        "CREATE TABLE IF NOT EXISTS sessions ("
        "  id INTEGER PRIMARY KEY AUTOINCREMENT,"
        "  timestamp INTEGER NOT NULL,"
        "  duration_ms INTEGER NOT NULL,"
        "  text TEXT NOT NULL,"
        "  char_count INTEGER NOT NULL,"
        "  word_count INTEGER NOT NULL"
        ");";

    char *errMsg = NULL;
    if (sqlite3_exec(_db, sql, NULL, NULL, &errMsg) != SQLITE_OK) {
        NSLog(@"[VelikiyMoguchiy] Failed to create sessions table: %s", errMsg);
        sqlite3_free(errMsg);
    }

    [self migrateSchema];
    sqlite3_create_function_v2(_db, "sp_contains", 2, SQLITE_UTF8 | SQLITE_DETERMINISTIC, NULL,
                               sqliteContains, NULL, NULL, NULL);
}

/// Add columns introduced after the initial release. Databases created before
/// the migration lack them; ALTER TABLE is a no-op-safe way to catch up.
/// `asr_text` / `asr_provider` stay NULL for sessions recorded before the
/// upgrade — downstream consumers (dictionary suggestions) must skip those.
- (void)migrateSchema {
    NSSet<NSString *> *existing = [self sessionColumnNames];

    NSDictionary<NSString *, NSString *> *wanted = @{
        @"asr_text" : @"TEXT",
        @"asr_provider" : @"TEXT",
        @"llm_applied" : @"INTEGER NOT NULL DEFAULT 0",
        @"processed_for_dictionary" : @"INTEGER NOT NULL DEFAULT 0",
        @"audio_path" : @"TEXT",
        @"status" : @"TEXT NOT NULL DEFAULT 'ok'",
        @"error" : @"TEXT",
        @"app_bundle_id" : @"TEXT",
        @"original_text" : @"TEXT",
        @"purpose" : @"TEXT",
    };

    for (NSString *column in wanted) {
        if ([existing containsObject:column]) continue;

        NSString *alter = [NSString stringWithFormat:@"ALTER TABLE sessions ADD COLUMN %@ %@;",
                           column, wanted[column]];
        char *errMsg = NULL;
        if (sqlite3_exec(_db, alter.UTF8String, NULL, NULL, &errMsg) != SQLITE_OK) {
            NSLog(@"[VelikiyMoguchiy] Failed to add column %@: %s", column, errMsg);
            sqlite3_free(errMsg);
        } else {
            NSLog(@"[VelikiyMoguchiy] History schema migrated: added column %@", column);
        }
    }
}

- (NSSet<NSString *> *)sessionColumnNames {
    NSMutableSet<NSString *> *columns = [NSMutableSet set];
    sqlite3_stmt *stmt = NULL;
    if (sqlite3_prepare_v2(_db, "PRAGMA table_info(sessions);", -1, &stmt, NULL) == SQLITE_OK) {
        while (sqlite3_step(stmt) == SQLITE_ROW) {
            const char *name = (const char *)sqlite3_column_text(stmt, 1);
            if (name) [columns addObject:[NSString stringWithUTF8String:name]];
        }
    }
    sqlite3_finalize(stmt);
    return columns;
}

- (void)recordSessionWithDurationMs:(NSInteger)durationMs
                               text:(NSString *)text
                            asrText:(NSString *)asrText
                        asrProvider:(NSString *)asrProvider
                         llmApplied:(BOOL)llmApplied {
    [self recordSessionWithDurationMs:durationMs
                                 text:text
                              asrText:asrText
                          asrProvider:asrProvider
                           llmApplied:llmApplied
                               status:@"ok"
                            audioPath:nil];
}

- (void)recordSessionWithDurationMs:(NSInteger)durationMs
                               text:(NSString *)text
                            asrText:(NSString *)asrText
                        asrProvider:(NSString *)asrProvider
                         llmApplied:(BOOL)llmApplied
                             status:(NSString *)status
                          audioPath:(NSString *)audioPath {
    [self recordSessionWithDurationMs:durationMs
                                 text:text
                              asrText:asrText
                          asrProvider:asrProvider
                           llmApplied:llmApplied
                               status:status
                            audioPath:audioPath
                                error:nil
                          appBundleId:nil];
}

static void bindOptionalText(sqlite3_stmt *stmt, int index, NSString *value) {
    if (value.length > 0) {
        sqlite3_bind_text(stmt, index, value.UTF8String, -1, SQLITE_TRANSIENT);
    } else {
        sqlite3_bind_null(stmt, index);
    }
}

- (int64_t)recordSessionWithDurationMs:(NSInteger)durationMs
                                  text:(NSString *)text
                               asrText:(NSString *)asrText
                           asrProvider:(NSString *)asrProvider
                            llmApplied:(BOOL)llmApplied
                                status:(NSString *)status
                             audioPath:(NSString *)audioPath
                                 error:(NSString *)error
                           appBundleId:(NSString *)appBundleId {
    return [self recordSessionWithDurationMs:durationMs
                                        text:text
                                     asrText:asrText
                                 asrProvider:asrProvider
                                  llmApplied:llmApplied
                                      status:status
                                   audioPath:audioPath
                                       error:error
                                 appBundleId:appBundleId
                                originalText:nil];
}

- (int64_t)recordSessionWithDurationMs:(NSInteger)durationMs
                                  text:(NSString *)text
                               asrText:(NSString *)asrText
                           asrProvider:(NSString *)asrProvider
                            llmApplied:(BOOL)llmApplied
                                status:(NSString *)status
                             audioPath:(NSString *)audioPath
                                 error:(NSString *)error
                           appBundleId:(NSString *)appBundleId
                          originalText:(NSString *)originalText {
    return [self recordSessionWithDurationMs:durationMs
                                        text:text
                                     asrText:asrText
                                 asrProvider:asrProvider
                                  llmApplied:llmApplied
                                      status:status
                                   audioPath:audioPath
                                       error:error
                                 appBundleId:appBundleId
                                originalText:originalText
                                     purpose:nil];
}

- (int64_t)recordSessionWithDurationMs:(NSInteger)durationMs
                                  text:(NSString *)text
                               asrText:(NSString *)asrText
                           asrProvider:(NSString *)asrProvider
                            llmApplied:(BOOL)llmApplied
                                status:(NSString *)status
                             audioPath:(NSString *)audioPath
                                 error:(NSString *)error
                           appBundleId:(NSString *)appBundleId
                          originalText:(NSString *)originalText
                               purpose:(NSString *)purpose {
    if (!_db) return 0;
    text = text ?: @"";
    status = status.length > 0 ? status : @"ok";
    // A session without text is only worth a row when it carries a recording.
    if (text.length == 0 && audioPath.length == 0) return 0;

    NSInteger charCount = 0;
    NSInteger wordCount = 0;
    [self countText:text charCount:&charCount wordCount:&wordCount];

    const char *sql = "INSERT INTO sessions (timestamp, duration_ms, text, char_count, word_count, "
                      "asr_text, asr_provider, llm_applied, audio_path, status, error, app_bundle_id, "
                      "original_text, purpose) "
                      "VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?);";
    sqlite3_stmt *stmt = NULL;
    int64_t rowId = 0;

    if (sqlite3_prepare_v2(_db, sql, -1, &stmt, NULL) == SQLITE_OK) {
        sqlite3_bind_int64(stmt, 1, (sqlite3_int64)[[NSDate date] timeIntervalSince1970]);
        sqlite3_bind_int64(stmt, 2, (sqlite3_int64)durationMs);
        sqlite3_bind_text(stmt, 3, text.UTF8String, -1, SQLITE_TRANSIENT);
        sqlite3_bind_int64(stmt, 4, (sqlite3_int64)charCount);
        sqlite3_bind_int64(stmt, 5, (sqlite3_int64)wordCount);
        bindOptionalText(stmt, 6, asrText);
        bindOptionalText(stmt, 7, asrProvider);
        sqlite3_bind_int(stmt, 8, llmApplied ? 1 : 0);
        bindOptionalText(stmt, 9, audioPath);
        sqlite3_bind_text(stmt, 10, status.UTF8String, -1, SQLITE_TRANSIENT);
        bindOptionalText(stmt, 11, [SPHistoryManager scrubbedSecrets:error]);
        bindOptionalText(stmt, 12, appBundleId);
        bindOptionalText(stmt, 13, originalText);
        bindOptionalText(stmt, 14, [purpose isEqualToString:SPHistoryPurposeDictate] ? nil : purpose);

        if (sqlite3_step(stmt) == SQLITE_DONE) {
            rowId = (int64_t)sqlite3_last_insert_rowid(_db);
        } else {
            NSLog(@"[VelikiyMoguchiy] Failed to insert session: %s", sqlite3_errmsg(_db));
        }
    }
    sqlite3_finalize(stmt);

    NSLog(@"[VelikiyMoguchiy] History recorded — duration:%ldms chars:%ld words:%ld provider:%@ llm:%d status:%@",
          (long)durationMs, (long)charCount, (long)wordCount,
          asrProvider.length > 0 ? asrProvider : @"?", llmApplied ? 1 : 0, status);

    if (rowId > 0) [self postChangeNotification];
    return rowId;
}

- (BOOL)updateSessionWithId:(int64_t)sessionId
                       text:(NSString *)text
                     status:(NSString *)status
                      error:(NSString *)error {
    if (!_db || sessionId <= 0) return NO;

    NSInteger charCount = 0;
    NSInteger wordCount = 0;
    if (text) [self countText:text charCount:&charCount wordCount:&wordCount];

    const char *sql =
        "UPDATE sessions SET "
        "original_text = CASE WHEN ?1 IS NOT NULL AND original_text IS NULL "
        "                     AND text != '' AND text != ?1 THEN text ELSE original_text END, "
        "asr_text = CASE WHEN ?1 IS NOT NULL AND text != ?1 THEN NULL ELSE asr_text END, "
        "text = COALESCE(?1, text), "
        "char_count = CASE WHEN ?1 IS NOT NULL THEN ?2 ELSE char_count END, "
        "word_count = CASE WHEN ?1 IS NOT NULL THEN ?3 ELSE word_count END, "
        "status = COALESCE(?4, status), "
        "error = ?5 "
        "WHERE id = ?6;";
    sqlite3_stmt *stmt = NULL;
    BOOL updated = NO;
    if (sqlite3_prepare_v2(_db, sql, -1, &stmt, NULL) == SQLITE_OK) {
        if (text) {
            sqlite3_bind_text(stmt, 1, text.UTF8String, -1, SQLITE_TRANSIENT);
        } else {
            sqlite3_bind_null(stmt, 1);
        }
        sqlite3_bind_int64(stmt, 2, (sqlite3_int64)charCount);
        sqlite3_bind_int64(stmt, 3, (sqlite3_int64)wordCount);
        bindOptionalText(stmt, 4, status);
        bindOptionalText(stmt, 5, [SPHistoryManager scrubbedSecrets:error]);
        sqlite3_bind_int64(stmt, 6, (sqlite3_int64)sessionId);
        if (sqlite3_step(stmt) == SQLITE_DONE) {
            updated = sqlite3_changes(_db) > 0;
        } else {
            NSLog(@"[VelikiyMoguchiy] Failed to update session: %s", sqlite3_errmsg(_db));
        }
    }
    sqlite3_finalize(stmt);
    if (updated) [self postChangeNotification];
    return updated;
}

#pragma mark - Error reasons

static const NSUInteger kProviderMessageLimit = 300;

static NSString *truncated(NSString *text, NSUInteger limit) {
    if (text.length <= limit) return text;
    NSRange safe = [text rangeOfComposedCharacterSequencesForRange:NSMakeRange(0, limit)];
    return [[text substringWithRange:safe] stringByAppendingString:@"…"];
}

+ (NSString *)scrubbedSecrets:(NSString *)message {
    if (message.length == 0) return message;
    static NSRegularExpression *keys;
    static NSRegularExpression *bearer;
    static dispatch_once_t once;
    dispatch_once(&once, ^{
        keys = [NSRegularExpression regularExpressionWithPattern:@"(?<![A-Za-z0-9])sk-[A-Za-z0-9_-]+"
                                                         options:0 error:nil];
        bearer = [NSRegularExpression regularExpressionWithPattern:@"(bearer\\s+)[A-Za-z0-9._~+/=-]+"
                                                           options:NSRegularExpressionCaseInsensitive
                                                             error:nil];
    });
    NSMutableString *result = [message mutableCopy];
    [keys replaceMatchesInString:result options:0 range:NSMakeRange(0, result.length)
                    withTemplate:@"sk-***"];
    [bearer replaceMatchesInString:result options:0 range:NSMakeRange(0, result.length)
                      withTemplate:@"$1***"];
    return result;
}

+ (NSString *)errorReasonWithSummary:(NSString *)summary providerMessage:(NSString *)providerMessage {
    NSCharacterSet *trim = [NSCharacterSet whitespaceAndNewlineCharacterSet];
    NSString *head = [[self scrubbedSecrets:summary] ?: @"" stringByTrimmingCharactersInSet:trim];
    head = [[head componentsSeparatedByCharactersInSet:[NSCharacterSet newlineCharacterSet]]
            componentsJoinedByString:@" "];
    NSString *detail = [[self scrubbedSecrets:providerMessage] ?: @"" stringByTrimmingCharactersInSet:trim];
    detail = truncated(detail, kProviderMessageLimit);
    if (head.length == 0) return detail;
    if (detail.length == 0 || [detail isEqualToString:head]) return head;
    return [NSString stringWithFormat:@"%@\n%@", head, detail];
}

+ (NSString *)purposeForCode:(NSInteger)code {
    switch (code) {
        case 1: return SPHistoryPurposeTranslate;
        case 2: return SPHistoryPurposeAsk;
        case 3: return SPHistoryPurposeEdit;
        default: return SPHistoryPurposeDictate;
    }
}

+ (NSString *)titleForPurpose:(NSString *)purpose {
    if ([purpose isEqualToString:SPHistoryPurposeTranslate]) return @"перевод";
    if ([purpose isEqualToString:SPHistoryPurposeAsk]) return @"вопрос";
    if ([purpose isEqualToString:SPHistoryPurposeEdit]) return @"правка";
    return @"диктовка";
}

+ (NSString *)shortErrorReason:(NSString *)error {
    if (error.length == 0) return @"";
    NSRange newline = [error rangeOfCharacterFromSet:[NSCharacterSet newlineCharacterSet]];
    return newline.location == NSNotFound ? error : [error substringToIndex:newline.location];
}

- (void)countText:(NSString *)text charCount:(NSInteger *)outChars wordCount:(NSInteger *)outWords {
    NSInteger chars = 0;
    NSInteger words = 0;
    BOOL inWord = NO;

    for (NSUInteger i = 0; i < text.length; i++) {
        unichar ch = [text characterAtIndex:i];

        // CJK Unified Ideographs and extensions
        if ((ch >= 0x4E00 && ch <= 0x9FFF) ||   // CJK Unified
            (ch >= 0x3400 && ch <= 0x4DBF) ||   // CJK Extension A
            (ch >= 0xF900 && ch <= 0xFAFF)) {   // CJK Compatibility
            chars++;
            if (inWord) {
                words++;
                inWord = NO;
            }
        } else if ((ch >= 'A' && ch <= 'Z') || (ch >= 'a' && ch <= 'z') ||
                   (ch >= '0' && ch <= '9') || ch == '\'') {
            // Latin alphanumeric — part of a word
            if (!inWord) {
                inWord = YES;
            }
        } else {
            // Whitespace, punctuation, etc.
            if (inWord) {
                words++;
                inWord = NO;
            }
        }
    }
    if (inWord) {
        words++;
    }

    *outChars = chars;
    *outWords = words;
}

- (SPHistoryStats *)aggregateStats {
    SPHistoryStats *stats = [[SPHistoryStats alloc] init];
    if (!_db) return stats;

    const char *sql = "SELECT COUNT(*), COALESCE(SUM(duration_ms),0), "
                      "COALESCE(SUM(char_count),0), COALESCE(SUM(word_count),0) "
                      "FROM sessions WHERE status = 'ok' AND COALESCE(purpose, 'dictate') NOT IN ('ask', 'edit');";
    sqlite3_stmt *stmt = NULL;

    if (sqlite3_prepare_v2(_db, sql, -1, &stmt, NULL) == SQLITE_OK) {
        if (sqlite3_step(stmt) == SQLITE_ROW) {
            stats.sessionCount = (NSInteger)sqlite3_column_int64(stmt, 0);
            stats.totalDurationMs = (NSInteger)sqlite3_column_int64(stmt, 1);
            stats.totalCharCount = (NSInteger)sqlite3_column_int64(stmt, 2);
            stats.totalWordCount = (NSInteger)sqlite3_column_int64(stmt, 3);
        }
    }
    sqlite3_finalize(stmt);

    return stats;
}

static NSString *const kSessionColumns =
    @"id, timestamp, duration_ms, text, status, audio_path, error, app_bundle_id, original_text, purpose";

static NSString *columnString(sqlite3_stmt *stmt, int column, NSString *fallback) {
    const char *value = (const char *)sqlite3_column_text(stmt, column);
    return value ? ([NSString stringWithUTF8String:value] ?: fallback) : fallback;
}

/// Rows of a prepared SELECT of `kSessionColumns`.
static NSArray<NSDictionary *> *sessionRows(sqlite3_stmt *stmt) {
    NSMutableArray<NSDictionary *> *sessions = [NSMutableArray array];
    while (sqlite3_step(stmt) == SQLITE_ROW) {
        if (!sqlite3_column_text(stmt, 3)) continue;
        NSTimeInterval ts = (NSTimeInterval)sqlite3_column_int64(stmt, 1);
        NSString *purpose = columnString(stmt, 9, @"");
        [sessions addObject:@{
            @"id" : @(sqlite3_column_int64(stmt, 0)),
            @"timestamp" : [NSDate dateWithTimeIntervalSince1970:ts],
            @"duration_ms" : @(sqlite3_column_int64(stmt, 2)),
            @"text" : columnString(stmt, 3, @""),
            @"status" : columnString(stmt, 4, @"ok"),
            @"audio_path" : columnString(stmt, 5, @""),
            @"error" : columnString(stmt, 6, @""),
            @"app_bundle_id" : columnString(stmt, 7, @""),
            @"original_text" : columnString(stmt, 8, @""),
            @"purpose" : purpose.length > 0 ? purpose : SPHistoryPurposeDictate,
        }];
    }
    return sessions;
}

- (NSArray<NSDictionary *> *)recentSessionsWithLimit:(NSUInteger)limit {
    if (!_db) return @[];
    NSString *sql = [NSString stringWithFormat:@"SELECT %@ FROM sessions ORDER BY id DESC LIMIT ?;",
                     kSessionColumns];
    sqlite3_stmt *stmt = NULL;
    NSArray<NSDictionary *> *sessions = @[];
    if (sqlite3_prepare_v2(_db, sql.UTF8String, -1, &stmt, NULL) == SQLITE_OK) {
        sqlite3_bind_int64(stmt, 1, (sqlite3_int64)limit);
        sessions = sessionRows(stmt);
    }
    sqlite3_finalize(stmt);
    return sessions;
}

- (NSArray<NSDictionary *> *)sessionsMatchingQuery:(NSString *)query limit:(NSUInteger)limit {
    if (!_db || query.length == 0) return @[];
    NSString *sql = [NSString stringWithFormat:
        @"SELECT %@ FROM sessions "
        @"WHERE sp_contains(text, ?1) OR sp_contains(original_text, ?1) "
        @"ORDER BY id DESC LIMIT ?2;", kSessionColumns];
    sqlite3_stmt *stmt = NULL;
    NSArray<NSDictionary *> *sessions = @[];
    if (sqlite3_prepare_v2(_db, sql.UTF8String, -1, &stmt, NULL) == SQLITE_OK) {
        sqlite3_bind_text(stmt, 1, query.UTF8String, -1, SQLITE_TRANSIENT);
        sqlite3_bind_int64(stmt, 2, (sqlite3_int64)limit);
        sessions = sessionRows(stmt);
    } else {
        NSLog(@"[VelikiyMoguchiy] History search failed: %s", sqlite3_errmsg(_db));
    }
    sqlite3_finalize(stmt);
    return sessions;
}

- (NSInteger)sessionCount {
    if (!_db) return 0;
    sqlite3_stmt *stmt = NULL;
    NSInteger count = 0;
    if (sqlite3_prepare_v2(_db, "SELECT COUNT(*) FROM sessions;", -1, &stmt, NULL) == SQLITE_OK &&
        sqlite3_step(stmt) == SQLITE_ROW) {
        count = (NSInteger)sqlite3_column_int64(stmt, 0);
    }
    sqlite3_finalize(stmt);
    return count;
}

- (void)deleteSessionWithId:(int64_t)sessionId {
    if (!_db) return;
    sqlite3_stmt *stmt = NULL;
    if (sqlite3_prepare_v2(_db, "DELETE FROM sessions WHERE id = ?;", -1, &stmt, NULL) == SQLITE_OK) {
        sqlite3_bind_int64(stmt, 1, sessionId);
        if (sqlite3_step(stmt) != SQLITE_DONE) {
            NSLog(@"[VelikiyMoguchiy] Failed to delete session: %s", sqlite3_errmsg(_db));
        }
    }
    sqlite3_finalize(stmt);
    [self postChangeNotification];
}

- (void)deleteAllSessions {
    if (!_db) return;
    char *errMsg = NULL;
    // VACUUM rewrites the file, so the deleted texts do not linger in free pages.
    if (sqlite3_exec(_db, "DELETE FROM sessions; VACUUM;", NULL, NULL, &errMsg) != SQLITE_OK) {
        NSLog(@"[VelikiyMoguchiy] Failed to clear history: %s", errMsg);
        sqlite3_free(errMsg);
    }
    [self postChangeNotification];
}

- (void)detachAudioPath:(NSString *)audioPath {
    if (!_db || audioPath.length == 0) return;
    // Rows that exist only for their recording (cancelled / failed / empty)
    // have nothing left to show once the file is gone.
    const char *statements[] = {
        "DELETE FROM sessions WHERE audio_path = ? AND text = '';",
        "UPDATE sessions SET audio_path = NULL WHERE audio_path = ?;",
    };
    for (size_t i = 0; i < sizeof(statements) / sizeof(statements[0]); i++) {
        sqlite3_stmt *stmt = NULL;
        if (sqlite3_prepare_v2(_db, statements[i], -1, &stmt, NULL) == SQLITE_OK) {
            sqlite3_bind_text(stmt, 1, audioPath.UTF8String, -1, SQLITE_TRANSIENT);
            sqlite3_step(stmt);
        }
        sqlite3_finalize(stmt);
    }
    [self postChangeNotification];
}

- (void)postChangeNotification {
    dispatch_async(dispatch_get_main_queue(), ^{
        [[NSNotificationCenter defaultCenter] postNotificationName:SPHistoryDidChangeNotification
                                                            object:self];
    });
}

- (void)dealloc {
    if (_db) {
        sqlite3_close(_db);
        _db = NULL;
    }
}

@end
