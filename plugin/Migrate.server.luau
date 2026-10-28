local HttpService = game:GetService("HttpService")
local ChangeHistoryService = game:GetService("ChangeHistoryService")
local ScriptEditorService = game:GetService("ScriptEditorService")
local AssetService = game:GetService("AssetService")

local serverUrl = "http://127.0.0.1:48620"
local muted = Color3.fromRGB(136, 136, 136)
local bright = Color3.fromRGB(224, 224, 224)
local green = Color3.fromRGB(59, 186, 87)
local red = Color3.fromRGB(208, 96, 112)

local watched = {
	Sound = { "SoundId" },
	MeshPart = { "MeshId", "TextureID" },
	SpecialMesh = { "MeshId", "TextureId" },
	FileMesh = { "MeshId", "TextureId" },
	Decal = { "Texture" },
	Texture = { "Texture" },
	ImageLabel = { "Image", "HoverImage", "PressedImage" },
	ImageButton = { "Image", "HoverImage", "PressedImage" },
	ParticleEmitter = { "Texture" },
	Beam = { "Texture" },
	Trail = { "Texture" },
	Animation = { "AnimationId" },
	Shirt = { "ShirtTemplate" },
	Pants = { "PantsTemplate" },
	ShirtGraphic = { "Graphic" },
	Tool = { "TextureId" },
	Sky = { "SkyboxBk", "SkyboxDn", "SkyboxFt", "SkyboxLf", "SkyboxRt", "SkyboxUp", "SunTextureId", "MoonTextureId" },
}

local token = plugin:GetSetting("rbxyoinkerToken") or ""
local refs = {}
local scriptRefs = {}
local lastMap = {}
local seenRun = -1
local polling = false

local toolbar = plugin:CreateToolbar("rbxyoinker")
local toggle = toolbar:CreateButton("Migrate", "Move this place's assets to another group with rbxyoinker", "")
toggle.ClickableWhenViewportHidden = true

local widget = plugin:CreateDockWidgetPluginGui(
	"RbxyoinkerMigrate",
	DockWidgetPluginGuiInfo.new(Enum.InitialDockState.Right, false, false, 300, 260, 260, 220)
)
widget.Title = "rbxyoinker"

local root = Instance.new("Frame")
root.Size = UDim2.fromScale(1, 1)
root.BackgroundColor3 = Color3.fromRGB(22, 22, 22)
root.BorderSizePixel = 0
root.Parent = widget

local padding = Instance.new("UIPadding")
padding.PaddingTop = UDim.new(0, 14)
padding.PaddingBottom = UDim.new(0, 14)
padding.PaddingLeft = UDim.new(0, 14)
padding.PaddingRight = UDim.new(0, 14)
padding.Parent = root

local layout = Instance.new("UIListLayout")
layout.Padding = UDim.new(0, 10)
layout.SortOrder = Enum.SortOrder.LayoutOrder
layout.Parent = root

local title = Instance.new("TextLabel")
title.BackgroundTransparency = 1
title.Size = UDim2.new(1, 0, 0, 20)
title.Font = Enum.Font.GothamBold
title.RichText = true
title.Text = 'rbx<font color="#3BEA57">yoinker</font> <font color="#888888">migrate</font>'
title.TextColor3 = Color3.fromRGB(255, 255, 255)
title.TextSize = 15
title.TextXAlignment = Enum.TextXAlignment.Left
title.LayoutOrder = 1
title.Parent = root

local info = Instance.new("TextLabel")
info.BackgroundTransparency = 1
info.Size = UDim2.fromScale(1, 0)
info.AutomaticSize = Enum.AutomaticSize.Y
info.Font = Enum.Font.Gotham
info.TextColor3 = muted
info.TextSize = 12
info.TextWrapped = true
info.TextXAlignment = Enum.TextXAlignment.Left
info.LayoutOrder = 2
info.Parent = root

local codeBox = Instance.new("TextBox")
codeBox.Size = UDim2.new(1, 0, 0, 34)
codeBox.BackgroundColor3 = Color3.fromRGB(16, 16, 16)
codeBox.ClearTextOnFocus = false
codeBox.Font = Enum.Font.Code
codeBox.PlaceholderText = "6 digit code"
codeBox.PlaceholderColor3 = Color3.fromRGB(85, 85, 85)
codeBox.Text = ""
codeBox.TextColor3 = Color3.fromRGB(255, 255, 255)
codeBox.TextSize = 16
codeBox.LayoutOrder = 3
codeBox.Parent = root
local boxCorner = Instance.new("UICorner")
boxCorner.CornerRadius = UDim.new(0, 6)
boxCorner.Parent = codeBox
local boxStroke = Instance.new("UIStroke")
boxStroke.ApplyStrokeMode = Enum.ApplyStrokeMode.Border
boxStroke.Color = Color3.fromRGB(50, 52, 56)
boxStroke.Parent = codeBox

local mainButton = Instance.new("TextButton")
mainButton.Size = UDim2.new(1, 0, 0, 34)
mainButton.BackgroundColor3 = Color3.fromRGB(36, 38, 40)
mainButton.AutoButtonColor = true
mainButton.Font = Enum.Font.GothamMedium
mainButton.TextColor3 = bright
mainButton.TextSize = 13
mainButton.LayoutOrder = 4
mainButton.Parent = root
boxCorner:Clone().Parent = mainButton
local buttonStroke = boxStroke:Clone()
buttonStroke.Color = Color3.fromRGB(60, 62, 66)
buttonStroke.Parent = mainButton

local applyButton = mainButton:Clone()
applyButton.Text = "Swap ids again"
applyButton.LayoutOrder = 5
applyButton.Visible = false
applyButton.Parent = root

local track = Instance.new("Frame")
track.Size = UDim2.new(1, 0, 0, 4)
track.BackgroundColor3 = Color3.fromRGB(40, 40, 40)
track.BorderSizePixel = 0
track.LayoutOrder = 6
track.Parent = root
local trackCorner = Instance.new("UICorner")
trackCorner.CornerRadius = UDim.new(1, 0)
trackCorner.Parent = track

local fill = Instance.new("Frame")
fill.Size = UDim2.fromScale(0, 1)
fill.BackgroundColor3 = Color3.fromRGB(59, 234, 87)
fill.BorderSizePixel = 0
fill.Parent = track
trackCorner:Clone().Parent = fill

local status = Instance.new("TextLabel")
status.BackgroundTransparency = 1
status.Size = UDim2.fromScale(1, 0)
status.AutomaticSize = Enum.AutomaticSize.Y
status.Font = Enum.Font.Gotham
status.TextColor3 = muted
status.TextSize = 12
status.TextWrapped = true
status.TextXAlignment = Enum.TextXAlignment.Left
status.LayoutOrder = 7
status.Parent = root

local disconnectButton = Instance.new("TextButton")
disconnectButton.BackgroundTransparency = 1
disconnectButton.Size = UDim2.new(1, 0, 0, 16)
disconnectButton.Font = Enum.Font.Gotham
disconnectButton.Text = "Disconnect"
disconnectButton.TextColor3 = Color3.fromRGB(102, 102, 102)
disconnectButton.TextSize = 12
disconnectButton.TextXAlignment = Enum.TextXAlignment.Left
disconnectButton.LayoutOrder = 8
disconnectButton.Parent = root

local function setStatus(text, color)
	status.Text = text
	status.TextColor3 = color or muted
end

local function showView()
	local paired = token ~= ""
	codeBox.Visible = not paired
	track.Visible = paired
	disconnectButton.Visible = paired
	applyButton.Visible = paired and next(lastMap) ~= nil
	if paired then
		info.Text = "Scan sends every asset id in this place to rbxyoinker. Pick where they go in the app's Migrate tab, this panel swaps the new ids in when it's done."
		mainButton.Text = "Scan place"
	else
		info.Text = "Open the Migrate tab in rbxyoinker, click Get a code and type it here."
		mainButton.Text = "Connect"
		fill.Size = UDim2.fromScale(0, 1)
	end
end

local function forget()
	token = ""
	plugin:SetSetting("rbxyoinkerToken", "")
	polling = false
	showView()
end

local function call(method, path, body)
	local ok, res = pcall(HttpService.RequestAsync, HttpService, {
		Url = serverUrl .. path,
		Method = method,
		Headers = { ["Content-Type"] = "application/json", ["X-Yoinker-Token"] = token },
		Body = body and HttpService:JSONEncode(body) or nil,
	})
	if not ok then
		setStatus("Can't reach rbxyoinker. Start the app and try again.", red)
		return nil
	end
	local decodedOk, decoded = pcall(HttpService.JSONDecode, HttpService, res.Body)
	if res.StatusCode == 401 then
		forget()
		setStatus("rbxyoinker forgot this Studio. Get a new code in the Migrate tab.", red)
		return nil
	end
	if not decodedOk or not decoded.ok then
		setStatus(decodedOk and tostring(decoded.error) or "rbxyoinker sent back something odd.", red)
		return nil
	end
	return decoded.data
end

local function scan()
	refs = {}
	scriptRefs = {}
	local ids = {}
	local seen = {}
	for _, inst in game:GetDescendants() do
		local props = watched[inst.ClassName]
		if props then
			for _, prop in props do
				local ok, value = pcall(function()
					return inst[prop]
				end)
				local digits = ok and typeof(value) == "string" and string.match(value, "%d%d%d+")
				if digits then
					table.insert(refs, { instance = inst, prop = prop, id = digits })
					if not seen[digits] then
						seen[digits] = true
						table.insert(ids, digits)
					end
				end
			end
		elseif inst:IsA("LuaSourceContainer") then
			local ok, source = pcall(function()
				return inst.Source
			end)
			if ok and typeof(source) == "string" then
				local found = false
				for _, pattern in { "rbxassetid://(%d+)", "asset/%?id=(%d+)" } do
					for digits in string.gmatch(source, pattern) do
						found = true
						if not seen[digits] then
							seen[digits] = true
							table.insert(ids, digits)
						end
					end
				end
				if found then
					table.insert(scriptRefs, inst)
				end
			end
		end
	end
	return ids
end

local function applySwaps()
	local swapped = 0
	local waiting = 0
	local meshParts = {}
	local recording = ChangeHistoryService:TryBeginRecording("rbxyoinker migrate")
	for _, ref in refs do
		local newId = lastMap[ref.id]
		if newId and ref.instance.Parent then
			local content = "rbxassetid://" .. newId
			if ref.instance:IsA("MeshPart") and ref.prop == "MeshId" then
				local source = meshParts[newId]
				if not source then
					local ok, part = pcall(AssetService.CreateMeshPartAsync, AssetService, content)
					if ok then
						source = part
						meshParts[newId] = part
					end
				end
				if source and pcall(ref.instance.ApplyMesh, ref.instance, source) then
					ref.id = newId
					swapped += 1
				else
					waiting += 1
				end
			elseif pcall(function()
				ref.instance[ref.prop] = content
			end) then
				ref.id = newId
				swapped += 1
			end
		end
	end
	for _, container in scriptRefs do
		if container.Parent then
			pcall(ScriptEditorService.UpdateSourceAsync, ScriptEditorService, container, function(source)
				for oldId, newId in lastMap do
					local first, firstCount = string.gsub(source, "rbxassetid://" .. oldId .. "%f[%D]", "rbxassetid://" .. newId)
					local second, secondCount = string.gsub(first, "asset/%?id=" .. oldId .. "%f[%D]", "asset/?id=" .. newId)
					source = second
					swapped += firstCount + secondCount
				end
				return source
			end)
		end
	end
	if recording then
		ChangeHistoryService:FinishRecording(recording, Enum.FinishRecordingOperation.Commit)
	end
	if waiting > 0 then
		setStatus(swapped .. " ids swapped. " .. waiting .. " meshes aren't processed by Roblox yet, click Swap ids again in a minute.", bright)
	else
		setStatus(swapped .. " ids swapped. Ctrl+Z undoes it.", green)
	end
	applyButton.Visible = true
end

local function poll()
	if polling or token == "" then
		return
	end
	polling = true
	task.spawn(function()
		while polling and widget.Enabled and token ~= "" do
			local job = call("GET", "/api/studio/job")
			if not job then
				break
			end
			local total = 0
			local done = 0
			for _, item in job.items do
				if item.state ~= "found" then
					total += 1
					if item.state == "ok" or item.state == "error" then
						done += 1
					end
				end
			end
			fill.Size = UDim2.fromScale(total > 0 and done / total or 0, 1)
			if job.running then
				setStatus("Moving " .. done .. " / " .. total .. " ...", bright)
			elseif seenRun >= 0 and job.run_id ~= seenRun and total > 0 then
				seenRun = job.run_id
				lastMap = {}
				for _, item in job.items do
					if item.new_id then
						lastMap[tostring(item.id)] = tostring(item.new_id)
					end
				end
				if job.mode == "swap" then
					applySwaps()
				else
					setStatus("Done. Copy the mapping from the app.", green)
				end
			end
			task.wait(2)
		end
		polling = false
	end)
end

local function connect()
	local code = string.gsub(codeBox.Text, "%D", "")
	if #code ~= 6 then
		setStatus("The code is 6 digits.", red)
		return
	end
	local paired = call("POST", "/api/studio/pair", { code = code })
	if not paired then
		return
	end
	token = paired.token
	plugin:SetSetting("rbxyoinkerToken", token)
	codeBox.Text = ""
	showView()
	setStatus("Connected. Scan the place when you're ready.", green)
	poll()
end

local function scanAndSend()
	setStatus("Scanning ...")
	local ids = scan()
	if #ids == 0 then
		setStatus("No asset ids found in this place.")
		return
	end
	local sent = call("POST", "/api/studio/submit", { place = game.Name, ids = ids })
	if not sent then
		return
	end
	seenRun = sent.run_id
	lastMap = {}
	applyButton.Visible = false
	fill.Size = UDim2.fromScale(0, 1)
	setStatus("Sent " .. sent.count .. " ids. Pick where they go in rbxyoinker's Migrate tab.", bright)
	poll()
end

mainButton.MouseButton1Click:Connect(function()
	if token == "" then
		connect()
	else
		scanAndSend()
	end
end)

codeBox.FocusLost:Connect(function(enterPressed)
	if enterPressed then
		connect()
	end
end)

applyButton.MouseButton1Click:Connect(applySwaps)

disconnectButton.MouseButton1Click:Connect(function()
	forget()
	setStatus("Disconnected.")
end)

toggle.Click:Connect(function()
	widget.Enabled = not widget.Enabled
end)

widget:GetPropertyChangedSignal("Enabled"):Connect(function()
	if widget.Enabled then
		poll()
	end
end)

showView()
setStatus(token ~= "" and "Connected to rbxyoinker." or "The rbxyoinker app has to be running.")
if widget.Enabled then
	poll()
end
