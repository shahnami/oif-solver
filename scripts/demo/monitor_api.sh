#!/bin/bash
# monitor_api.sh - Real-time monitoring of the OIF Solver Service API

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
MAGENTA='\033[0;35m'
NC='\033[0m' # No Color

# Configuration
API_BASE="http://localhost:8080"
REFRESH_INTERVAL=3

# Function to clear screen and show header
show_header() {
    clear
    echo -e "${BLUE}🚀 OIF Solver Service - Real-time Monitor${NC}"
    echo -e "${BLUE}===========================================${NC}"
    echo -e "${CYAN}API: $API_BASE | Refresh: ${REFRESH_INTERVAL}s | Time: $(date '+%H:%M:%S')${NC}"
    echo ""
}

# Function to check API status
check_api_status() {
    if curl -s -f "$API_BASE/health" > /dev/null 2>&1; then
        echo -e "${GREEN}✅ API Status: ONLINE${NC}"
        return 0
    else
        echo -e "${RED}❌ API Status: OFFLINE${NC}"
        return 1
    fi
}

# Function to get and display discovery status
show_discovery_status() {
    echo -e "${YELLOW}📡 Discovery Status${NC}"
    echo "==================="
    
    local response=$(curl -s "$API_BASE/api/v1/discovery/status" 2>/dev/null)
    
    if [ $? -eq 0 ] && [ -n "$response" ]; then
        local is_running=$(echo "$response" | jq -r '.is_running // false' 2>/dev/null)
        local sources=$(echo "$response" | jq -r '.sources // {}' 2>/dev/null)
        
        if [ "$is_running" = "true" ]; then
            echo -e "${GREEN}🟢 Discovery Manager: RUNNING${NC}"
        else
            echo -e "${RED}🔴 Discovery Manager: STOPPED${NC}"
        fi
        
        # Show source details
        if [ "$sources" != "{}" ] && [ "$sources" != "null" ]; then
            echo -e "${BLUE}Sources:${NC}"
            echo "$sources" | jq -r 'to_entries[] | "  \(.key): \(.value.status // "unknown")"' 2>/dev/null || echo "  Unable to parse sources"
        else
            echo -e "${YELLOW}  No active sources${NC}"
        fi
    else
        echo -e "${RED}❌ Unable to fetch discovery status${NC}"
    fi
    echo ""
}

# Function to get and display discovery statistics
show_discovery_stats() {
    echo -e "${YELLOW}📊 Discovery Statistics${NC}"
    echo "======================="
    
    local response=$(curl -s "$API_BASE/api/v1/discovery/stats" 2>/dev/null)
    
    if [ $? -eq 0 ] && [ -n "$response" ]; then
        local total_events=$(echo "$response" | jq -r '.total_events_discovered // 0' 2>/dev/null)
        local active_sources=$(echo "$response" | jq -r '.total_sources_active // 0' 2>/dev/null)
        local total_errors=$(echo "$response" | jq -r '.total_errors // 0' 2>/dev/null)
        local events_per_min=$(echo "$response" | jq -r '.events_per_minute // 0' 2>/dev/null)
        local duplicates=$(echo "$response" | jq -r '.duplicate_events_filtered // 0' 2>/dev/null)
        local last_activity=$(echo "$response" | jq -r '.last_activity_timestamp // null' 2>/dev/null)
        
        echo -e "${CYAN}  Total Events Discovered: $total_events${NC}"
        echo -e "${CYAN}  Active Sources: $active_sources${NC}"
        echo -e "${CYAN}  Events per Minute: $(printf "%.2f" $events_per_min)${NC}"
        echo -e "${CYAN}  Total Errors: $total_errors${NC}"
        echo -e "${CYAN}  Duplicates Filtered: $duplicates${NC}"
        
        if [ "$last_activity" != "null" ] && [ -n "$last_activity" ]; then
            local activity_time=$(date -d "@$last_activity" '+%H:%M:%S' 2>/dev/null || echo "unknown")
            echo -e "${CYAN}  Last Activity: $activity_time${NC}"
        else
            echo -e "${CYAN}  Last Activity: None${NC}"
        fi
    else
        echo -e "${RED}❌ Unable to fetch discovery statistics${NC}"
    fi
    echo ""
}

# Function to show plugin health
show_plugin_health() {
    echo -e "${YELLOW}🔌 Plugin Health${NC}"
    echo "==============="
    
    local response=$(curl -s "$API_BASE/api/v1/plugins/health" 2>/dev/null)
    
    if [ $? -eq 0 ] && [ -n "$response" ]; then
        local summary=$(echo "$response" | jq -r '.summary // {}' 2>/dev/null)
        local healthy=$(echo "$summary" | jq -r '.healthy_plugins // 0' 2>/dev/null)
        local unhealthy=$(echo "$summary" | jq -r '.unhealthy_plugins // 0' 2>/dev/null)
        
        echo -e "${GREEN}  Healthy Plugins: $healthy${NC}"
        echo -e "${RED}  Unhealthy Plugins: $unhealthy${NC}"
        
        # Show individual plugin status
        local plugins=$(echo "$response" | jq -r '.plugins // {}' 2>/dev/null)
        if [ "$plugins" != "{}" ] && [ "$plugins" != "null" ]; then
            echo -e "${BLUE}  Plugin Details:${NC}"
            echo "$plugins" | jq -r 'to_entries[] | "    \(.key): \(.value.status // "unknown")"' 2>/dev/null || echo "    Unable to parse plugin details"
        fi
    else
        echo -e "${RED}❌ Unable to fetch plugin health${NC}"
    fi
    echo ""
}

# Function to show recent events (if implemented)
show_recent_events() {
    echo -e "${YELLOW}📋 Recent Events${NC}"
    echo "================"
    
    local response=$(curl -s "$API_BASE/api/v1/events/recent?limit=5" 2>/dev/null)
    
    if [ $? -eq 0 ] && [ -n "$response" ]; then
        local events=$(echo "$response" | jq -r '. // []' 2>/dev/null)
        
        if [ "$events" != "[]" ] && [ "$events" != "null" ]; then
            echo "$events" | jq -r '.[] | "  \(.timestamp | todate): \(.event_type) - \(.id)"' 2>/dev/null || echo "  Unable to parse events"
        else
            echo -e "${CYAN}  No recent events${NC}"
        fi
    else
        echo -e "${CYAN}  Recent events endpoint not available${NC}"
    fi
    echo ""
}

# Function to show solver service logs (if available)
show_solver_logs() {
    echo -e "${YELLOW}📝 Recent Solver Logs${NC}"
    echo "====================="
    
    # Try to show last few log lines if solver is running locally
    if pgrep -f "solver-service" > /dev/null 2>&1; then
        echo -e "${GREEN}  Solver process is running${NC}"
        
        # Try to find recent logs in various locations
        local log_found=false
        
        # Check for RUST_LOG output in current directory
        if [ -f "solver.log" ]; then
            echo -e "${BLUE}  Last 3 log entries:${NC}"
            tail -n 3 solver.log | sed 's/^/    /' || true
            log_found=true
        fi
        
        if [ "$log_found" = false ]; then
            echo -e "${CYAN}  No log file found (check terminal output)${NC}"
        fi
    else
        echo -e "${YELLOW}  Solver process not running${NC}"
        echo -e "${BLUE}  Start with: cargo run --bin solver-service${NC}"
    fi
    echo ""
}

# Function to show network status
show_network_status() {
    echo -e "${YELLOW}🌐 Network Status${NC}"
    echo "================"
    
    # Check if Anvil is running
    if curl -s -X POST -H "Content-Type: application/json" \
        --data '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}' \
        http://localhost:8545 > /dev/null 2>&1; then
        
        # Get block number
        local block_response=$(curl -s -X POST -H "Content-Type: application/json" \
            --data '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}' \
            http://localhost:8545)
        local block_hex=$(echo "$block_response" | jq -r '.result // "0x0"' 2>/dev/null)
        local block_number=$(printf "%d" "$block_hex" 2>/dev/null || echo "0")
        
        echo -e "${GREEN}  Anvil: RUNNING${NC}"
        echo -e "${CYAN}  Current Block: $block_number${NC}"
        echo -e "${CYAN}  RPC: http://localhost:8545${NC}"
    else
        echo -e "${RED}  Anvil: NOT RUNNING${NC}"
        echo -e "${BLUE}  Start with: ./setup_local_anvil.sh${NC}"
    fi
    echo ""
}

# Function to show helpful commands
show_commands() {
    echo -e "${YELLOW}🛠️  Helpful Commands${NC}"
    echo "==================="
    echo -e "${BLUE}  Send Intent:     ${CYAN}./send_intent.sh${NC}"
    echo -e "${BLUE}  Check Balances:  ${CYAN}./send_intent.sh balances${NC}"
    echo -e "${BLUE}  Health Check:    ${CYAN}curl $API_BASE/health${NC}"
    echo -e "${BLUE}  Start Discovery: ${CYAN}curl -X POST $API_BASE/api/v1/discovery/start${NC}"
    echo -e "${BLUE}  Stop Discovery:  ${CYAN}curl -X POST $API_BASE/api/v1/discovery/stop${NC}"
    echo ""
}

# Function for continuous monitoring
monitor_continuous() {
    echo -e "${GREEN}🔄 Starting continuous monitoring (Press Ctrl+C to stop)${NC}"
    echo -e "${YELLOW}⏱️  Refreshing every $REFRESH_INTERVAL seconds${NC}"
    echo ""
    
    # Trap to handle cleanup
    trap 'echo -e "\n${YELLOW}👋 Monitoring stopped${NC}"; exit 0' INT TERM
    
    while true; do
        show_header
        
        if check_api_status; then
            show_discovery_status
            show_discovery_stats
            show_plugin_health
            show_recent_events
            show_network_status
            show_solver_logs
            show_commands
        else
            echo ""
            echo -e "${RED}🚨 Solver API is not responding!${NC}"
            echo -e "${YELLOW}💡 Make sure to start the solver with:${NC}"
            echo -e "${BLUE}   cargo run --bin solver-service${NC}"
            echo ""
            show_network_status
        fi
        
        echo -e "${MAGENTA}$(printf '=%.0s' {1..45})${NC}"
        echo -e "${CYAN}Refreshing in $REFRESH_INTERVAL seconds... (Ctrl+C to stop)${NC}"
        
        sleep $REFRESH_INTERVAL
    done
}

# Function for one-time status check
check_status() {
    show_header
    
    if check_api_status; then
        show_discovery_status
        show_discovery_stats
        show_plugin_health
        show_recent_events
        show_network_status
        show_solver_logs
        show_commands
    else
        echo ""
        echo -e "${RED}🚨 Solver API is not responding!${NC}"
        echo -e "${YELLOW}💡 Make sure to start the solver with:${NC}"
        echo -e "${BLUE}   cargo run --bin solver-service${NC}"
        echo ""
        show_network_status
    fi
}

# Function to test specific endpoints
test_endpoints() {
    echo -e "${BLUE}🧪 Testing API Endpoints${NC}"
    echo "========================"
    
    local endpoints=(
        "/health:Health Check"
        "/api/v1/discovery/status:Discovery Status"
        "/api/v1/discovery/stats:Discovery Stats"
        "/api/v1/plugins/health:Plugin Health"
        "/api/v1/events/recent:Recent Events"
    )
    
    for endpoint_info in "${endpoints[@]}"; do
        IFS=':' read -r endpoint description <<< "$endpoint_info"
        
        echo -e "${YELLOW}Testing $description...${NC}"
        
        local status_code=$(curl -s -o /dev/null -w "%{http_code}" "$API_BASE$endpoint" 2>/dev/null)
        
        if [ "$status_code" = "200" ]; then
            echo -e "${GREEN}  ✅ $endpoint - HTTP $status_code${NC}"
        elif [ "$status_code" = "404" ]; then
            echo -e "${YELLOW}  ⚠️  $endpoint - HTTP $status_code (Not Implemented)${NC}"
        else
            echo -e "${RED}  ❌ $endpoint - HTTP $status_code${NC}"
        fi
    done
    echo ""
}

# Function to show usage
show_usage() {
    echo "Usage: $0 [command]"
    echo ""
    echo "Commands:"
    echo "  monitor (default) - Continuous real-time monitoring"
    echo "  status           - One-time status check"
    echo "  test             - Test all API endpoints"
    echo "  discovery        - Only discovery information"
    echo "  plugins          - Only plugin health"
    echo "  network          - Only network status"
    echo "  -h, --help       - Show this help"
    echo ""
    echo "Environment variables:"
    echo "  REFRESH_INTERVAL - Refresh interval in seconds (default: 3)"
    echo "  API_BASE         - API base URL (default: http://localhost:8080)"
    exit 0
}

# Handle environment variables
if [ -n "$MONITOR_REFRESH_INTERVAL" ]; then
    REFRESH_INTERVAL=$MONITOR_REFRESH_INTERVAL
fi

if [ -n "$MONITOR_API_BASE" ]; then
    API_BASE=$MONITOR_API_BASE
fi

# Handle help
if [ "$1" = "-h" ] || [ "$1" = "--help" ]; then
    show_usage
fi

# Main execution
COMMAND="${1:-monitor}"

case "$COMMAND" in
    "monitor")
        monitor_continuous
        ;;
    "status")
        check_status
        ;;
    "test")
        test_endpoints
        ;;
    "discovery")
        show_header
        check_api_status
        show_discovery_status
        show_discovery_stats
        ;;
    "plugins")
        show_header
        check_api_status
        show_plugin_health
        ;;
    "network")
        show_header
        show_network_status
        ;;
    *)
        echo -e "${RED}❌ Unknown command: $COMMAND${NC}"
        show_usage
        ;;
esac