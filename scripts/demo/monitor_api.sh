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
METRICS_BASE="http://localhost:9090"
REFRESH_INTERVAL=3

# Function to clear screen and show header
show_header() {
    clear
    echo -e "${BLUE}🚀 OIF Solver Service - Real-time Monitor${NC}"
    echo -e "${BLUE}===========================================${NC}"
    echo -e "${CYAN}API: $API_BASE | Metrics: $METRICS_BASE | Refresh: ${REFRESH_INTERVAL}s | Time: $(date '+%H:%M:%S')${NC}"
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

# Function to get and display service health
show_service_health() {
    echo -e "${YELLOW}🏥 Service Health${NC}"
    echo "================="
    
    local response=$(curl -s "$API_BASE/health" 2>/dev/null)
    
    if [ $? -eq 0 ] && [ -n "$response" ]; then
        local status=$(echo "$response" | jq -r '.status // "unknown"' 2>/dev/null)
        local services=$(echo "$response" | jq -r '.services // {}' 2>/dev/null)
        
        # Overall status
        case "$status" in
            "healthy")
                echo -e "${GREEN}🟢 Overall Status: HEALTHY${NC}"
                ;;
            "degraded")
                echo -e "${YELLOW}🟡 Overall Status: DEGRADED${NC}"
                ;;
            "unhealthy")
                echo -e "${RED}🔴 Overall Status: UNHEALTHY${NC}"
                ;;
            "starting")
                echo -e "${BLUE}🔵 Overall Status: STARTING${NC}"
                ;;
            "stopping")
                echo -e "${MAGENTA}🟣 Overall Status: STOPPING${NC}"
                ;;
            *)
                echo -e "${CYAN}⚪ Overall Status: $status${NC}"
                ;;
        esac
        
        # Individual service status
        echo -e "${BLUE}Service Components:${NC}"
        local discovery=$(echo "$services" | jq -r '.discovery // false' 2>/dev/null)
        local delivery=$(echo "$services" | jq -r '.delivery // false' 2>/dev/null)
        local state=$(echo "$services" | jq -r '.state // false' 2>/dev/null)
        local event_processor=$(echo "$services" | jq -r '.event_processor // false' 2>/dev/null)
        
        [ "$discovery" = "true" ] && echo -e "  Discovery:        ${GREEN}✓${NC}" || echo -e "  Discovery:        ${RED}✗${NC}"
        [ "$delivery" = "true" ] && echo -e "  Delivery:         ${GREEN}✓${NC}" || echo -e "  Delivery:         ${RED}✗${NC}"
        [ "$state" = "true" ] && echo -e "  State:            ${GREEN}✓${NC}" || echo -e "  State:            ${RED}✗${NC}"
        [ "$event_processor" = "true" ] && echo -e "  Event Processor:  ${GREEN}✓${NC}" || echo -e "  Event Processor:  ${RED}✗${NC}"
    else
        echo -e "${RED}❌ Unable to fetch service health${NC}"
    fi
    echo ""
}

# Function to check readiness
show_readiness() {
    echo -e "${YELLOW}🚦 Readiness Status${NC}"
    echo "=================="
    
    local live_status=$(curl -s -o /dev/null -w "%{http_code}" "$API_BASE/health/live" 2>/dev/null)
    local ready_status=$(curl -s -o /dev/null -w "%{http_code}" "$API_BASE/health/ready" 2>/dev/null)
    
    if [ "$live_status" = "200" ]; then
        echo -e "${GREEN}  Liveness:  ✅ ALIVE${NC}"
    else
        echo -e "${RED}  Liveness:  ❌ NOT ALIVE${NC}"
    fi
    
    if [ "$ready_status" = "200" ]; then
        echo -e "${GREEN}  Readiness: ✅ READY${NC}"
    else
        echo -e "${YELLOW}  Readiness: ⚠️  NOT READY${NC}"
    fi
    echo ""
}

# Function to show current configuration
show_config() {
    echo -e "${YELLOW}⚙️  Configuration${NC}"
    echo "==============="
    
    local response=$(curl -s "$API_BASE/api/v1/admin/config" 2>/dev/null)
    
    if [ $? -eq 0 ] && [ -n "$response" ]; then
        # Extract solver address from order plugin config
        local solver_address=$(echo "$response" | jq -r '.plugins.order // {} | to_entries[0].value.config.solver_address // "unknown"' 2>/dev/null)
        
        # Extract unique chain IDs from all plugin configs
        local unique_chains=$(echo "$response" | jq -r '[
            (.plugins.discovery // {} | to_entries[].value.config.chain_id // empty),
            (.plugins.delivery // {} | to_entries[].value.config.chain_id // empty)
        ] | unique | length' 2>/dev/null || echo "0")
        
        local plugins=$(echo "$response" | jq -r '.plugins // {}' 2>/dev/null)
        
        if [ "$solver_address" != "unknown" ]; then
            echo -e "${CYAN}  Solver Address: ${solver_address:0:10}...${solver_address: -8}${NC}"
        else
            echo -e "${YELLOW}  Solver Address: Not configured${NC}"
        fi
        
        echo -e "${CYAN}  Configured Chains: $unique_chains${NC}"
        
        # Show plugin types with enabled status
        local discovery_count=$(echo "$plugins" | jq '.discovery // {} | to_entries | map(select(.value.enabled == true)) | length' 2>/dev/null || echo "0")
        local delivery_count=$(echo "$plugins" | jq '.delivery // {} | to_entries | map(select(.value.enabled == true)) | length' 2>/dev/null || echo "0")
        local settlement_count=$(echo "$plugins" | jq '.settlement // {} | to_entries | map(select(.value.enabled == true)) | length' 2>/dev/null || echo "0")
        local order_count=$(echo "$plugins" | jq '.order // {} | to_entries | map(select(.value.enabled == true)) | length' 2>/dev/null || echo "0")
        local state_count=$(echo "$plugins" | jq '.state // {} | to_entries | map(select(.value.enabled == true)) | length' 2>/dev/null || echo "0")
        
        echo -e "${CYAN}  Discovery Plugins: $discovery_count enabled${NC}"
        echo -e "${CYAN}  Delivery Plugins: $delivery_count enabled${NC}"
        echo -e "${CYAN}  Settlement Plugins: $settlement_count enabled${NC}"
        echo -e "${CYAN}  Order Plugins: $order_count enabled${NC}"
        echo -e "${CYAN}  State Plugins: $state_count enabled${NC}"
    else
        echo -e "${RED}❌ Unable to fetch configuration${NC}"
    fi
    echo ""
}

# Function to show metrics (if available)
show_metrics() {
    echo -e "${YELLOW}📊 Metrics${NC}"
    echo "========="
    
    local response=$(curl -s "$METRICS_BASE/metrics" 2>/dev/null)
    
    if [ $? -eq 0 ] && [ -n "$response" ]; then
        # Parse simple metrics
        local solver_health=$(echo "$response" | grep "solver_health" | grep -v "#" | awk '{print $2}' 2>/dev/null || echo "0")
        
        if [ "$solver_health" = "1" ]; then
            echo -e "${GREEN}  Solver Health Metric: 1 (Healthy)${NC}"
        else
            echo -e "${RED}  Solver Health Metric: $solver_health${NC}"
        fi
        
        # Add more metrics parsing as they become available
    else
        echo -e "${CYAN}  Metrics endpoint not available${NC}"
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
    echo -e "${BLUE}  Send Intent:      ${CYAN}./scripts/demo/send_intent.sh${NC}"
    echo -e "${BLUE}  Check Balances:   ${CYAN}./scripts/demo/send_intent.sh balances${NC}"
    echo -e "${BLUE}  Health Check:     ${CYAN}curl $API_BASE/health | jq${NC}"
    echo -e "${BLUE}  Get Order:        ${CYAN}curl $API_BASE/api/v1/orders/{order_id} | jq${NC}"
    echo -e "${BLUE}  Get Settlement:   ${CYAN}curl $API_BASE/api/v1/settlements/{settlement_id} | jq${NC}"
    echo -e "${BLUE}  View Config:      ${CYAN}curl $API_BASE/api/v1/admin/config | jq${NC}"
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
            show_service_health
            show_readiness
            show_config
            show_metrics
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
        show_service_health
        show_readiness
        show_config
        show_metrics
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
        "/health/live:Liveness Probe"
        "/health/ready:Readiness Probe"
        "/api/v1/admin/config:Configuration"
    )
    
    for endpoint_info in "${endpoints[@]}"; do
        IFS=':' read -r endpoint description <<< "$endpoint_info"
        
        echo -e "${YELLOW}Testing $description...${NC}"
        
        local status_code=$(curl -s -o /dev/null -w "%{http_code}" "$API_BASE$endpoint" 2>/dev/null)
        
        if [ "$status_code" = "200" ]; then
            echo -e "${GREEN}  ✅ $endpoint - HTTP $status_code${NC}"
        elif [ "$status_code" = "404" ]; then
            echo -e "${YELLOW}  ⚠️  $endpoint - HTTP $status_code (Not Found)${NC}"
        elif [ "$status_code" = "503" ]; then
            echo -e "${YELLOW}  ⚠️  $endpoint - HTTP $status_code (Service Unavailable)${NC}"
        else
            echo -e "${RED}  ❌ $endpoint - HTTP $status_code${NC}"
        fi
    done
    
    # Test metrics endpoint separately
    echo -e "${YELLOW}Testing Metrics endpoint...${NC}"
    local metrics_status=$(curl -s -o /dev/null -w "%{http_code}" "$METRICS_BASE/metrics" 2>/dev/null)
    if [ "$metrics_status" = "200" ]; then
        echo -e "${GREEN}  ✅ /metrics - HTTP $metrics_status (on port 9090)${NC}"
    else
        echo -e "${CYAN}  ℹ️  /metrics - HTTP $metrics_status (on port 9090)${NC}"
    fi
    
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
    echo "  health           - Only health information"
    echo "  config           - Only configuration"
    echo "  network          - Only network status"
    echo "  -h, --help       - Show this help"
    echo ""
    echo "Environment variables:"
    echo "  REFRESH_INTERVAL - Refresh interval in seconds (default: 3)"
    echo "  API_BASE         - API base URL (default: http://localhost:8080)"
    echo "  METRICS_BASE     - Metrics URL (default: http://localhost:9090)"
    exit 0
}

# Handle environment variables
if [ -n "$MONITOR_REFRESH_INTERVAL" ]; then
    REFRESH_INTERVAL=$MONITOR_REFRESH_INTERVAL
fi

if [ -n "$MONITOR_API_BASE" ]; then
    API_BASE=$MONITOR_API_BASE
fi

if [ -n "$MONITOR_METRICS_BASE" ]; then
    METRICS_BASE=$MONITOR_METRICS_BASE
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
    "health")
        show_header
        check_api_status
        show_service_health
        show_readiness
        ;;
    "config")
        show_header
        check_api_status
        show_config
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