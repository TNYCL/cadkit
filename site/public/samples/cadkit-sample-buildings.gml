<?xml version="1.0" encoding="UTF-8"?><core:CityModel xmlns:core="http://www.opengis.net/citygml/2.0" xmlns:bldg="http://www.opengis.net/citygml/building/2.0" xmlns:gen="http://www.opengis.net/citygml/generics/2.0" xmlns:gml="http://www.opengis.net/gml" xmlns:xlink="http://www.w3.org/1999/xlink">
<gml:description>Synthetic CityGML 2.0 sample by cadkit.org (MIT OR Apache-2.0). Fictional buildings at round coordinates.</gml:description>
<gml:name>cadkit sample buildings</gml:name>
<gml:boundedBy><gml:Envelope srsName="urn:ogc:def:crs,crs:EPSG::25832,crs:EPSG::7837" srsDimension="3"><gml:lowerCorner>500000 5600000 50</gml:lowerCorner><gml:upperCorner>500016 5600008 59</gml:upperCorner></gml:Envelope></gml:boundedBy>
<core:cityObjectMember>
<bldg:Building gml:id="ck-house">
<gml:description>Synthetic gable-roof house, 10 m x 8 m, eaves at 6 m, ridge at 9 m. Fictional; not a real building.</gml:description>
<gml:name>House</gml:name>
<gen:stringAttribute name="source"><gen:value>cadkit.org synthetic sample, MIT OR Apache-2.0</gen:value></gen:stringAttribute>
<bldg:measuredHeight uom="m">9</bldg:measuredHeight>
<bldg:storeysAboveGround>2</bldg:storeysAboveGround>
<bldg:lod2Solid><gml:Solid srsName="urn:ogc:def:crs,crs:EPSG::25832,crs:EPSG::7837" srsDimension="3"><gml:exterior><gml:CompositeSurface>
<gml:surfaceMember xlink:href="#ck-house-ground-poly"></gml:surfaceMember>
<gml:surfaceMember xlink:href="#ck-house-wall-s-poly"></gml:surfaceMember>
<gml:surfaceMember xlink:href="#ck-house-wall-e-poly"></gml:surfaceMember>
<gml:surfaceMember xlink:href="#ck-house-wall-n-poly"></gml:surfaceMember>
<gml:surfaceMember xlink:href="#ck-house-wall-w-poly"></gml:surfaceMember>
<gml:surfaceMember xlink:href="#ck-house-roof-s-poly"></gml:surfaceMember>
<gml:surfaceMember xlink:href="#ck-house-roof-n-poly"></gml:surfaceMember>
</gml:CompositeSurface></gml:exterior></gml:Solid></bldg:lod2Solid>
<bldg:boundedBy><bldg:GroundSurface gml:id="ck-house-ground"><bldg:lod2MultiSurface><gml:MultiSurface srsName="urn:ogc:def:crs,crs:EPSG::25832,crs:EPSG::7837" srsDimension="3"><gml:surfaceMember><gml:Polygon gml:id="ck-house-ground-poly"><gml:exterior><gml:LinearRing><gml:posList srsDimension="3">500000 5600000 50 500000 5600008 50 500010 5600008 50 500010 5600000 50 500000 5600000 50</gml:posList></gml:LinearRing></gml:exterior></gml:Polygon></gml:surfaceMember></gml:MultiSurface></bldg:lod2MultiSurface></bldg:GroundSurface></bldg:boundedBy>
<bldg:boundedBy><bldg:WallSurface gml:id="ck-house-wall-s"><bldg:lod2MultiSurface><gml:MultiSurface srsName="urn:ogc:def:crs,crs:EPSG::25832,crs:EPSG::7837" srsDimension="3"><gml:surfaceMember><gml:Polygon gml:id="ck-house-wall-s-poly"><gml:exterior><gml:LinearRing><gml:posList srsDimension="3">500000 5600000 50 500010 5600000 50 500010 5600000 56 500000 5600000 56 500000 5600000 50</gml:posList></gml:LinearRing></gml:exterior></gml:Polygon></gml:surfaceMember></gml:MultiSurface></bldg:lod2MultiSurface></bldg:WallSurface></bldg:boundedBy>
<bldg:boundedBy><bldg:WallSurface gml:id="ck-house-wall-e"><bldg:lod2MultiSurface><gml:MultiSurface srsName="urn:ogc:def:crs,crs:EPSG::25832,crs:EPSG::7837" srsDimension="3"><gml:surfaceMember><gml:Polygon gml:id="ck-house-wall-e-poly"><gml:exterior><gml:LinearRing><gml:posList srsDimension="3">500010 5600000 50 500010 5600008 50 500010 5600008 56 500010 5600004 59 500010 5600000 56 500010 5600000 50</gml:posList></gml:LinearRing></gml:exterior></gml:Polygon></gml:surfaceMember></gml:MultiSurface></bldg:lod2MultiSurface></bldg:WallSurface></bldg:boundedBy>
<bldg:boundedBy><bldg:WallSurface gml:id="ck-house-wall-n"><bldg:lod2MultiSurface><gml:MultiSurface srsName="urn:ogc:def:crs,crs:EPSG::25832,crs:EPSG::7837" srsDimension="3"><gml:surfaceMember><gml:Polygon gml:id="ck-house-wall-n-poly"><gml:exterior><gml:LinearRing><gml:posList srsDimension="3">500010 5600008 50 500000 5600008 50 500000 5600008 56 500010 5600008 56 500010 5600008 50</gml:posList></gml:LinearRing></gml:exterior></gml:Polygon></gml:surfaceMember></gml:MultiSurface></bldg:lod2MultiSurface></bldg:WallSurface></bldg:boundedBy>
<bldg:boundedBy><bldg:WallSurface gml:id="ck-house-wall-w"><bldg:lod2MultiSurface><gml:MultiSurface srsName="urn:ogc:def:crs,crs:EPSG::25832,crs:EPSG::7837" srsDimension="3"><gml:surfaceMember><gml:Polygon gml:id="ck-house-wall-w-poly"><gml:exterior><gml:LinearRing><gml:posList srsDimension="3">500000 5600008 50 500000 5600000 50 500000 5600000 56 500000 5600004 59 500000 5600008 56 500000 5600008 50</gml:posList></gml:LinearRing></gml:exterior></gml:Polygon></gml:surfaceMember></gml:MultiSurface></bldg:lod2MultiSurface></bldg:WallSurface></bldg:boundedBy>
<bldg:boundedBy><bldg:RoofSurface gml:id="ck-house-roof-s"><bldg:lod2MultiSurface><gml:MultiSurface srsName="urn:ogc:def:crs,crs:EPSG::25832,crs:EPSG::7837" srsDimension="3"><gml:surfaceMember><gml:Polygon gml:id="ck-house-roof-s-poly"><gml:exterior><gml:LinearRing><gml:posList srsDimension="3">500000 5600000 56 500010 5600000 56 500010 5600004 59 500000 5600004 59 500000 5600000 56</gml:posList></gml:LinearRing></gml:exterior></gml:Polygon></gml:surfaceMember></gml:MultiSurface></bldg:lod2MultiSurface></bldg:RoofSurface></bldg:boundedBy>
<bldg:boundedBy><bldg:RoofSurface gml:id="ck-house-roof-n"><bldg:lod2MultiSurface><gml:MultiSurface srsName="urn:ogc:def:crs,crs:EPSG::25832,crs:EPSG::7837" srsDimension="3"><gml:surfaceMember><gml:Polygon gml:id="ck-house-roof-n-poly"><gml:exterior><gml:LinearRing><gml:posList srsDimension="3">500010 5600008 56 500000 5600008 56 500000 5600004 59 500010 5600004 59 500010 5600008 56</gml:posList></gml:LinearRing></gml:exterior></gml:Polygon></gml:surfaceMember></gml:MultiSurface></bldg:lod2MultiSurface></bldg:RoofSurface></bldg:boundedBy>
</bldg:Building>
</core:cityObjectMember>
<core:cityObjectMember>
<bldg:Building gml:id="ck-garage">
<gml:description>Synthetic flat-roof garage, 4 m x 6 m, 3 m high. Fictional; not a real building.</gml:description>
<gml:name>Garage</gml:name>
<gen:stringAttribute name="source"><gen:value>cadkit.org synthetic sample, MIT OR Apache-2.0</gen:value></gen:stringAttribute>
<bldg:measuredHeight uom="m">3</bldg:measuredHeight>
<bldg:storeysAboveGround>1</bldg:storeysAboveGround>
<bldg:lod2Solid><gml:Solid srsName="urn:ogc:def:crs,crs:EPSG::25832,crs:EPSG::7837" srsDimension="3"><gml:exterior><gml:CompositeSurface>
<gml:surfaceMember xlink:href="#ck-garage-ground-poly"></gml:surfaceMember>
<gml:surfaceMember xlink:href="#ck-garage-wall-s-poly"></gml:surfaceMember>
<gml:surfaceMember xlink:href="#ck-garage-wall-e-poly"></gml:surfaceMember>
<gml:surfaceMember xlink:href="#ck-garage-wall-n-poly"></gml:surfaceMember>
<gml:surfaceMember xlink:href="#ck-garage-wall-w-poly"></gml:surfaceMember>
<gml:surfaceMember xlink:href="#ck-garage-roof-poly"></gml:surfaceMember>
</gml:CompositeSurface></gml:exterior></gml:Solid></bldg:lod2Solid>
<bldg:boundedBy><bldg:GroundSurface gml:id="ck-garage-ground"><bldg:lod2MultiSurface><gml:MultiSurface srsName="urn:ogc:def:crs,crs:EPSG::25832,crs:EPSG::7837" srsDimension="3"><gml:surfaceMember><gml:Polygon gml:id="ck-garage-ground-poly"><gml:exterior><gml:LinearRing><gml:posList srsDimension="3">500012 5600001 50 500012 5600007 50 500016 5600007 50 500016 5600001 50 500012 5600001 50</gml:posList></gml:LinearRing></gml:exterior></gml:Polygon></gml:surfaceMember></gml:MultiSurface></bldg:lod2MultiSurface></bldg:GroundSurface></bldg:boundedBy>
<bldg:boundedBy><bldg:WallSurface gml:id="ck-garage-wall-s"><bldg:lod2MultiSurface><gml:MultiSurface srsName="urn:ogc:def:crs,crs:EPSG::25832,crs:EPSG::7837" srsDimension="3"><gml:surfaceMember><gml:Polygon gml:id="ck-garage-wall-s-poly"><gml:exterior><gml:LinearRing><gml:posList srsDimension="3">500012 5600001 50 500016 5600001 50 500016 5600001 53 500012 5600001 53 500012 5600001 50</gml:posList></gml:LinearRing></gml:exterior></gml:Polygon></gml:surfaceMember></gml:MultiSurface></bldg:lod2MultiSurface></bldg:WallSurface></bldg:boundedBy>
<bldg:boundedBy><bldg:WallSurface gml:id="ck-garage-wall-e"><bldg:lod2MultiSurface><gml:MultiSurface srsName="urn:ogc:def:crs,crs:EPSG::25832,crs:EPSG::7837" srsDimension="3"><gml:surfaceMember><gml:Polygon gml:id="ck-garage-wall-e-poly"><gml:exterior><gml:LinearRing><gml:posList srsDimension="3">500016 5600001 50 500016 5600007 50 500016 5600007 53 500016 5600001 53 500016 5600001 50</gml:posList></gml:LinearRing></gml:exterior></gml:Polygon></gml:surfaceMember></gml:MultiSurface></bldg:lod2MultiSurface></bldg:WallSurface></bldg:boundedBy>
<bldg:boundedBy><bldg:WallSurface gml:id="ck-garage-wall-n"><bldg:lod2MultiSurface><gml:MultiSurface srsName="urn:ogc:def:crs,crs:EPSG::25832,crs:EPSG::7837" srsDimension="3"><gml:surfaceMember><gml:Polygon gml:id="ck-garage-wall-n-poly"><gml:exterior><gml:LinearRing><gml:posList srsDimension="3">500016 5600007 50 500012 5600007 50 500012 5600007 53 500016 5600007 53 500016 5600007 50</gml:posList></gml:LinearRing></gml:exterior></gml:Polygon></gml:surfaceMember></gml:MultiSurface></bldg:lod2MultiSurface></bldg:WallSurface></bldg:boundedBy>
<bldg:boundedBy><bldg:WallSurface gml:id="ck-garage-wall-w"><bldg:lod2MultiSurface><gml:MultiSurface srsName="urn:ogc:def:crs,crs:EPSG::25832,crs:EPSG::7837" srsDimension="3"><gml:surfaceMember><gml:Polygon gml:id="ck-garage-wall-w-poly"><gml:exterior><gml:LinearRing><gml:posList srsDimension="3">500012 5600007 50 500012 5600001 50 500012 5600001 53 500012 5600007 53 500012 5600007 50</gml:posList></gml:LinearRing></gml:exterior></gml:Polygon></gml:surfaceMember></gml:MultiSurface></bldg:lod2MultiSurface></bldg:WallSurface></bldg:boundedBy>
<bldg:boundedBy><bldg:RoofSurface gml:id="ck-garage-roof"><bldg:lod2MultiSurface><gml:MultiSurface srsName="urn:ogc:def:crs,crs:EPSG::25832,crs:EPSG::7837" srsDimension="3"><gml:surfaceMember><gml:Polygon gml:id="ck-garage-roof-poly"><gml:exterior><gml:LinearRing><gml:posList srsDimension="3">500012 5600001 53 500016 5600001 53 500016 5600007 53 500012 5600007 53 500012 5600001 53</gml:posList></gml:LinearRing></gml:exterior></gml:Polygon></gml:surfaceMember></gml:MultiSurface></bldg:lod2MultiSurface></bldg:RoofSurface></bldg:boundedBy>
</bldg:Building>
</core:cityObjectMember>
</core:CityModel>