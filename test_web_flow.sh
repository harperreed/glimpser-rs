#!/bin/bash
# Test the complete web interface flow

echo "🧪 Testing Glimpser Web Interface Flow"
echo "========================================"

# Test 1: Root page loads (login page)
echo -n "✓ Testing root page loads... "
if curl -s http://127.0.0.1:8080 | grep -q "Glimpser.*Login"; then
    echo "✅ PASS"
else
    echo "❌ FAIL"
    exit 1
fi

# Test 2: Login works and returns JWT
echo -n "✓ Testing login... "
TOKEN=$(curl -s -H "Content-Type: application/json" -d '{"email":"admin@test.com","password":"password123"}' http://127.0.0.1:8080/api/auth/login | jq -r '.access_token')
if [[ "$TOKEN" != "null" && "$TOKEN" != "" ]]; then
    echo "✅ PASS (token received)"
else
    echo "❌ FAIL (no token)"
    exit 1
fi

# Test 3: Dashboard page loads
echo -n "✓ Testing dashboard page loads... "
if curl -s http://127.0.0.1:8080/static/dashboard.html | grep -q "System Overview"; then
    echo "✅ PASS"
else
    echo "❌ FAIL"
    exit 1
fi

# Test 4: Admin page loads
echo -n "✓ Testing admin page loads... "
if curl -s http://127.0.0.1:8080/static/admin.html | grep -q "Admin Panel"; then
    echo "✅ PASS"
else
    echo "❌ FAIL"
    exit 1
fi

# Test 5: API health endpoint works with token
echo -n "✓ Testing authenticated API call... "
HEALTH_RESPONSE=$(curl -s -H "Authorization: Bearer $TOKEN" http://127.0.0.1:8080/api/health)
if echo "$HEALTH_RESPONSE" | grep -q "healthy"; then
    echo "✅ PASS"
else
    echo "❌ FAIL (response: $HEALTH_RESPONSE)"
    exit 1
fi

# Test 6: Streams API responds (should work now)
echo -n "✓ Testing streams API... "
STREAMS_RESPONSE=$(curl -s -H "Authorization: Bearer $TOKEN" http://127.0.0.1:8080/api/streams)
if [[ "$STREAMS_RESPONSE" == "[]" ]] || echo "$STREAMS_RESPONSE" | grep -q "\["; then
    echo "✅ PASS (empty array or streams list)"
elif echo "$STREAMS_RESPONSE" | grep -q "Authentication required"; then
    echo "⚠️  WARN (still auth required)"
else
    echo "❌ FAIL (response: $STREAMS_RESPONSE)"
fi

echo ""
echo "🎉 Web interface basic flow test complete!"
echo "You can now test manually at: http://127.0.0.1:8080"
echo "Login credentials: admin@test.com / password123"
