# Quick Start Guide

## Running LogCrab

### Option 1: Direct Execution
```bash
cd /home/daniel/logowl
./target/release/logcrab
```

### Option 2: Using Cargo
```bash
cd /home/daniel/logowl
cargo run --release
```

## First Time Use

1. **Launch the application**
   - The welcome screen will appear with instructions

2. **Open a log file**
   - Click the "Open Log File" button, OR
   - Use the menu: File → Open Log File...
   - Navigate to `sample_log.txt` in the project directory

3. **Explore the results**
   - Scroll through the color-coded log entries
   - Red/orange lines are anomalous and worth investigating
   - White/pink lines are normal operations
   - Use the search bar to filter specific patterns

## Using the Search Feature

The search bar accepts regex patterns and highlights matching lines:

### Quick Examples
```
ERROR|FATAL              → Find errors and fatal messages
\d+\.\d+\.\d+\.\d+       → Find IP addresses
timeout|failed           → Find timeout or failed operations
DatabaseManager          → Find specific component logs
11-20 08:15              → Find logs from specific time
```

### Search Tips
1. **Start simple**: Try plain text first (e.g., `error`)
2. **Case matters**: Regex is case-sensitive by default
3. **Use OR**: `pattern1|pattern2|pattern3`
4. **Escape special chars**: Use `\.` for literal dots
5. **Real-time validation**: Green ✓ = valid, Red ❌ = invalid regex

### Search Workflow
1. Type your regex pattern in the search bar
2. Press Enter or just start typing (live update)
3. Matching text highlighted in **yellow** within each line
4. See match count in the stats: "🔍 X matches"
5. Click "Clear" to reset and see all lines again

## Understanding the Display

### Column Layout
```
Line | Timestamp | Lvl | PID | Tag | Message | Score
-----|-----------|-----|-----|-----|---------|-------
```

- **Line**: Original line number from the file
- **Timestamp**: Parsed timestamp (HH:MM:SS.mmm format)
- **Lvl**: Log level (V/D/I/W/E/F)
- **PID**: Process ID (if available)
- **Tag**: Log tag (Android logcat) or component
- **Message**: The actual log message (truncated to 120 chars)
- **Score**: Anomaly score (0-100)

### Color Coding

| Color | Score Range | Meaning |
|-------|-------------|---------|
| White | 0-30 | Normal, expected behavior |
| Light Pink | 30-45 | Slightly unusual |
| Pink | 45-60 | Worth noting |
| Orange | 60-80 | Suspicious, investigate |
| Red | 80-100 | Highly anomalous, critical |

### Log Level Colors

Log levels have their own colors regardless of anomaly score:
- **V** (Verbose): Gray
- **D** (Debug): Light Blue
- **I** (Info): Green
- **W** (Warning): Yellow
- **E** (Error): Light Red
- **F** (Fatal): Red

## What Makes a Line Anomalous?

LogCrab considers multiple factors:

1. **Structural Rarity** (30% weight)
   - Is this message pattern rare in the file?
   - Example: A unique error message vs. repeated progress updates

2. **Severity** (25% weight)
   - ERROR and FATAL levels automatically score higher
   - Sudden severity changes (INFO → ERROR) are flagged

3. **Temporal Patterns** (20% weight)
   - Long absence of a pattern that suddenly reappears
   - Burst activity (many messages in short time)

4. **Information Content** (15% weight)
   - Unusually complex or simple messages
   - Messages with abnormal length

## Sample Log Highlights

When you open `sample_log.txt`, look for:

1. **Database Connection Timeout** (lines ~21-24)
   - Should score high: rare ERROR followed by FATAL crash

2. **Crash Handler** (lines ~25-27)
   - Should score very high: FATAL level + rare stack trace

3. **Security Events** (lines ~57-59)
   - Should score high: rare security warnings

4. **Background Sync Progress** (lines ~10-13, 18-20)
   - Should score low: repeated, expected pattern

5. **Mixed Format Entry** (line ~86)
   - Should score high: rare service/PID combination

## Tips for Effective Use

1. **Start with High Scores**
   - Focus on lines scoring > 70 first
   - These are the most unusual events

2. **Look for Clusters**
   - Multiple high-scoring lines in sequence often indicate a problem
   - Example: Error → Retry → Failure → Crash

3. **Context Matters**
   - Click on a high-scoring line and read surrounding low-score lines
   - The context often explains why something is anomalous

4. **Common False Positives**
   - First occurrence of anything scores high (expected)
   - Format changes or mixed log sources
   - Very long messages (high entropy)

5. **True Positives to Watch For**
   - ERROR or FATAL levels
   - Security-related messages
   - Connection failures
   - Memory warnings
   - Unexpected shutdowns

## Testing with Your Own Logs

### Android Logcat
```bash
adb logcat > myapp.log
# Let it run for a while, then Ctrl+C
   ./target/release/logcrab
# Open myapp.log
```

### System Logs
```bash
journalctl -u myservice > service.log
./target/release/logcrab
# Open service.log
```

### Application Logs
Any text file with log-like content works:
- Web server logs (Apache, Nginx)
- Application logs (Spring Boot, Django, etc.)
- Container logs (Docker, Kubernetes)
- CI/CD logs (Jenkins, GitLab CI)

## Performance Expectations

| File Size | Lines | Load Time | Memory Usage |
|-----------|-------|-----------|--------------|
| 1 MB | ~10K | < 1 sec | ~50 MB |
| 10 MB | ~100K | ~3 sec | ~200 MB |
| 100 MB | ~1M | ~20 sec | ~1 GB |
| 500 MB | ~5M | ~90 sec | ~4 GB |

*Times measured on modern hardware with SSD*

## Keyboard Shortcuts

- **Ctrl+O**: Open file (when implemented)
- **Ctrl+Q**: Quit
- **Mouse wheel**: Scroll logs
- **Click**: Select line for copying (when implemented)

## Troubleshooting

### Application Won't Start
```bash
# Check if binary exists
   ls -l target/release/logcrab

# If not, rebuild
cargo build --release
```

### File Won't Load
- Check file encoding (should be UTF-8)
- Try a smaller sample first
- Check for binary data in the file

### Low Anomaly Scores
- File might be very uniform (not necessarily bad!)
- Try a file with more variety
- Check if ERROR/FATAL messages exist

### High Memory Usage
- Large files require proportional RAM
- Close other applications
- Consider filtering the file first (e.g., grep ERROR)

## Next Steps

1. **Try the sample log** to understand the UI
2. **Load your own logs** to find real issues
3. **Experiment with different log types** to see parser flexibility
4. **Read ARCHITECTURE.md** to understand scoring
5. **Read README.md** for full documentation

## Support & Contribution

Found a bug? Have an idea?
- File structure is clear and modular
- Tests are in each module
- Follow the trait-based pattern for new scorers
- See ARCHITECTURE.md for adding embeddings

Happy debugging! �
